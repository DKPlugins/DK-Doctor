//! Rule `circular-gate`: a progression deadlock among global switches.
//!
//! A "gate" is the set of global switches that must be ON for a place (a map-event
//! page, a triggered common event) to run; when that place runs it may turn other
//! switches ON. The adapter records each ON write together with its gate as a
//! [`SwitchGate`](crate::ir::SwitchGate). This rule detects the case the existing
//! rules miss: a switch that IS written somewhere (so `uninitialized-symbols` stays
//! silent) yet can never actually be turned ON, because its only setters are locked
//! behind switches that transitively require it — a **cycle** of mutually-blocking
//! gates. The content behind such a cycle is unreachable (a soft-lock).
//!
//! Approach:
//! 1. A switch is **freely settable** if it is managed by a plugin
//!    (`declared_by_plugin`/`set_by_plugin`), written by an opaque script
//!    ([`Ir::script_written_switches`](crate::ir::Ir)), or has an ON setter with an
//!    empty gate. These seed a monotone fixpoint: a switch becomes **reachable**
//!    once some setter's whole gate is reachable.
//! 2. **Candidates** = switches that have an intended setter but never become
//!    reachable.
//! 3. Among candidates, build the block graph `S → G` (G is an unreachable gate
//!    switch of some setter of S) and report each **cycle** — one finding per
//!    strongly-connected cluster. A dead-end into a truly uninitialized switch is
//!    NOT a cycle (its root is `uninitialized-symbols`' job), so it is not flagged.
//!
//! Confidence `likely`, opt-in: a plugin command (356/357) that turns a switch on
//! is not tracked, so on plugin-heavy projects a "deadlock" may be broken at
//! runtime by a plugin. Variables are intentionally out of scope (too many write
//! sources) — this MVP is switch-only.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::{Location, SwitchGate};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};
use rustc_hash::{FxHashMap, FxHashSet};

/// Rule that finds progression deadlocks (circular switch gates).
pub struct CircularGate;

impl Rule for CircularGate {
    fn id(&self) -> &'static str {
        "circular-gate"
    }

    fn category(&self) -> Category {
        Category::Reference
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let ir = ctx.ir;
        if ir.switch_gates.is_empty() {
            return Vec::new();
        }

        // setter switch id → all its ON setters (with their gates).
        let mut setters: FxHashMap<u32, Vec<&SwitchGate>> = FxHashMap::default();
        for sg in &ir.switch_gates {
            setters.entry(sg.switch_id).or_default().push(sg);
        }

        // Universe = every switch that participates as a setter target or a gate.
        let mut universe: FxHashSet<u32> = setters.keys().copied().collect();
        for sg in &ir.switch_gates {
            universe.extend(sg.gate.iter().copied());
        }

        // A switch that could be turned on outside the gate model.
        let is_free = |s: u32| -> bool {
            let managed = ir
                .symbols
                .switches
                .get(&s)
                .is_some_and(|i| i.declared_by_plugin || i.set_by_plugin);
            managed
                || ir.script_written_switches.contains(&s)
                || setters
                    .get(&s)
                    .is_some_and(|v| v.iter().any(|g| g.gate.is_empty()))
        };

        // Monotone fixpoint: seed with all free switches, then propagate
        // "a setter whose whole gate is reachable makes its target reachable".
        let mut reachable: FxHashSet<u32> =
            universe.iter().copied().filter(|&s| is_free(s)).collect();
        let mut changed = true;
        while changed {
            changed = false;
            for (&s, gates) in &setters {
                if reachable.contains(&s) {
                    continue;
                }
                if gates
                    .iter()
                    .any(|g| g.gate.iter().all(|t| reachable.contains(t)))
                {
                    reachable.insert(s);
                    changed = true;
                }
            }
        }

        // Candidates: has an intended setter, yet is never reachable.
        let candidates: FxHashSet<u32> = setters
            .keys()
            .copied()
            .filter(|s| !reachable.contains(s))
            .collect();
        if candidates.is_empty() {
            return Vec::new();
        }

        // Block graph among candidates: S → G where G is an (unreachable) gate
        // switch of some setter of S. Self-edges (a switch gated by itself) are
        // kept — they are degenerate deadlocks.
        let mut graph: FxHashMap<u32, FxHashSet<u32>> = FxHashMap::default();
        for &s in &candidates {
            let mut succ = FxHashSet::default();
            for g in &setters[&s] {
                for &t in &g.gate {
                    if candidates.contains(&t) {
                        succ.insert(t);
                    }
                }
            }
            graph.insert(s, succ);
        }

        // Emit one finding per cyclic strongly-connected cluster. Iterating in
        // ascending id order, the first unvisited cycle member is its cluster's
        // minimum (the representative).
        let mut cand_sorted: Vec<u32> = candidates.iter().copied().collect();
        cand_sorted.sort_unstable();
        let mut visited: FxHashSet<u32> = FxHashSet::default();
        let mut findings = Vec::new();
        for &s in &cand_sorted {
            if visited.contains(&s) || !reaches(&graph, s, s) {
                continue;
            }
            let cycle: Vec<u32> = cand_sorted
                .iter()
                .copied()
                .filter(|&t| t == s || (reaches(&graph, s, t) && reaches(&graph, t, s)))
                .collect();
            for &t in &cycle {
                visited.insert(t);
            }
            let rep = cycle[0];
            let location = setters[&rep]
                .first()
                .map(|g| g.location.clone())
                .unwrap_or_else(|| Location::file_only("data/CommonEvents.json"));
            let references: Vec<Location> = cycle
                .iter()
                .flat_map(|t| setters[t].iter().map(|g| g.location.clone()))
                .collect();
            let name = ir
                .symbols
                .switches
                .get(&rep)
                .and_then(|i| i.name.clone())
                .filter(|n| !n.is_empty());
            findings.push(Finding {
                severity: Severity::Warning,
                category: Category::Reference,
                confidence: Confidence::Likely,
                location,
                message: Msg::CircularGate {
                    switch_id: rep,
                    name,
                    cycle,
                },
                references,
                rule: "circular-gate",
            });
        }
        findings
    }
}

/// Whether `to` is reachable from `from` in the block graph following ≥1 edge
/// (so `reaches(g, s, s)` is true only when `s` lies on a cycle).
fn reaches(graph: &FxHashMap<u32, FxHashSet<u32>>, from: u32, to: u32) -> bool {
    let mut stack: Vec<u32> = graph.get(&from).into_iter().flatten().copied().collect();
    let mut seen = FxHashSet::default();
    while let Some(n) = stack.pop() {
        if n == to {
            return true;
        }
        if seen.insert(n)
            && let Some(next) = graph.get(&n)
        {
            stack.extend(next.iter().copied());
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Engine, Ir, IrBuilder, Location, SwitchGate};

    fn gate(b: &mut IrBuilder, switch_id: u32, gate: Vec<u32>) {
        b.add_switch_gate(SwitchGate {
            switch_id,
            gate,
            location: Location::file_only("data/Map001.json"),
        });
    }

    fn cycle_ids(f: &Finding) -> Vec<u32> {
        match &f.message {
            Msg::CircularGate { cycle, .. } => cycle.clone(),
            _ => panic!("expected CircularGate"),
        }
    }

    #[test]
    fn flags_two_switch_deadlock() {
        // Switch 1 is set only behind gate {2}; switch 2 only behind gate {1}.
        // Neither can go first → deadlock.
        let mut b = Ir::builder(Engine::Mz);
        gate(&mut b, 1, vec![2]);
        gate(&mut b, 2, vec![1]);
        let ir = b.finish();
        let f = CircularGate.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1, "one deadlock cluster");
        assert_eq!(f[0].rule, "circular-gate");
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(f[0].confidence, Confidence::Likely);
        assert_eq!(cycle_ids(&f[0]), vec![1, 2]);
        assert!(matches!(
            f[0].message,
            Msg::CircularGate { switch_id: 1, .. }
        ));
    }

    #[test]
    fn free_setter_breaks_the_cycle() {
        // Switch 2 also has an unconditional (empty-gate) setter → freely settable
        // → the cycle is broken, nothing is flagged.
        let mut b = Ir::builder(Engine::Mz);
        gate(&mut b, 1, vec![2]);
        gate(&mut b, 2, vec![1]);
        gate(&mut b, 2, vec![]); // free setter for switch 2
        let ir = b.finish();
        assert!(CircularGate.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn plugin_managed_switch_is_not_deadlocked() {
        let mut b = Ir::builder(Engine::Mz);
        gate(&mut b, 1, vec![2]);
        gate(&mut b, 2, vec![1]);
        // Switch 2 is declared by a plugin → assumed settable at runtime.
        b.symbols_mut().mark_switch_declared_by_plugin(2);
        let ir = b.finish();
        assert!(CircularGate.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn script_written_switch_is_not_deadlocked() {
        let mut b = Ir::builder(Engine::Mz);
        gate(&mut b, 1, vec![2]);
        gate(&mut b, 2, vec![1]);
        b.mark_switch_script_written(2);
        let ir = b.finish();
        assert!(CircularGate.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn dead_end_into_uninitialized_is_not_a_cycle() {
        // Switch 1 is gated by 9, which nobody ever sets (no SwitchGate). Switch 1
        // is unreachable, but this is a dead-end (uninitialized-symbols flags 9),
        // not a cycle → circular-gate stays silent.
        let mut b = Ir::builder(Engine::Mz);
        gate(&mut b, 1, vec![9]);
        let ir = b.finish();
        assert!(CircularGate.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn self_gated_switch_is_flagged() {
        // A page gated by switch 5 that itself sets switch 5 ON — 5 can never turn on.
        let mut b = Ir::builder(Engine::Mz);
        gate(&mut b, 5, vec![5]);
        let ir = b.finish();
        let f = CircularGate.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert_eq!(cycle_ids(&f[0]), vec![5]);
    }

    #[test]
    fn reachable_chain_is_not_flagged() {
        // Switch 1 has a free setter; switch 2 is gated by 1 (reachable); switch 3
        // is gated by 2 (reachable). No deadlock.
        let mut b = Ir::builder(Engine::Mz);
        gate(&mut b, 1, vec![]);
        gate(&mut b, 2, vec![1]);
        gate(&mut b, 3, vec![2]);
        let ir = b.finish();
        assert!(CircularGate.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn no_switch_gates_no_findings() {
        let ir = Ir::builder(Engine::Mz).finish();
        assert!(CircularGate.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn three_switch_cycle_reported_once() {
        // 1←2←3←1 forms a single cluster → one finding listing all three.
        let mut b = Ir::builder(Engine::Mz);
        gate(&mut b, 1, vec![3]);
        gate(&mut b, 2, vec![1]);
        gate(&mut b, 3, vec![2]);
        let ir = b.finish();
        let f = CircularGate.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert_eq!(cycle_ids(&f[0]), vec![1, 2, 3]);
    }
}
