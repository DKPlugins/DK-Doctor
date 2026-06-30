//! Rule `cyclic-common-events`: a cycle in mutual calls between common events.
//!
//! Builds the CommonEvent→CommonEvent call graph from [`Edge::CallsCommonEvent`]
//! edges (command 117) whose source is a common event itself. Any cycle in this
//! graph means infinite synchronous recursion when triggered (command 117 calls
//! a common event synchronously, without a scheduler). The list of ids is
//! reported in cycle order.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::{Edge, Entity, EntityId, Ir};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};
use rustc_hash::{FxHashMap, FxHashSet};

/// Rule that finds cycles in mutual calls between common events.
pub struct CyclicCommonEvents;

impl Rule for CyclicCommonEvents {
    fn id(&self) -> &'static str {
        "cyclic-common-events"
    }

    fn category(&self) -> Category {
        Category::DeadCode
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let ir = ctx.ir;

        // Call graph: ce_id → set of called ce_ids (only 117 from a CE).
        let mut graph: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
        for rec in &ir.edges {
            if let Edge::CallsCommonEvent { common_event_id } = rec.edge
                && let Some(src) = ce_id_of(ir, rec.from)
            {
                let targets = graph.entry(src).or_default();
                if !targets.contains(&common_event_id) {
                    targets.push(common_event_id);
                }
            }
        }

        // Colored DFS: report each distinct cycle once. Cycles are normalized
        // (smallest id first) and deduplicated so one cycle is not emitted twice.
        // `done` carries over between roots so an already-explored subgraph is
        // never revisited (keeps the whole pass linear in nodes + edges).
        let mut findings = Vec::new();
        let mut reported: FxHashSet<Vec<u32>> = FxHashSet::default();
        let mut done: FxHashSet<u32> = FxHashSet::default();

        let mut roots: Vec<u32> = graph.keys().copied().collect();
        roots.sort_unstable();
        for root in roots {
            if done.contains(&root) {
                continue;
            }
            let mut stack: Vec<u32> = Vec::new();
            let mut on_stack: FxHashSet<u32> = FxHashSet::default();
            find_cycles(
                root,
                &graph,
                &mut stack,
                &mut on_stack,
                &mut done,
                &mut reported,
                &mut findings,
                ir,
            );
        }
        findings
    }
}

/// id of the common event, if the edge's source entity is a common event.
fn ce_id_of(ir: &Ir, from: EntityId) -> Option<u32> {
    match &ir.entity(from)?.kind {
        Entity::CommonEvent(ce) => Some(ce.id),
        _ => None,
    }
}

/// DFS (white/gray/black coloring) reporting cycles reachable from `node`.
///
/// `on_stack` is the gray set (nodes on the current path); a back edge to a gray
/// node is a cycle. `done` is the black set (fully-explored nodes): we never
/// re-descend into a black node. That bound is what keeps the traversal O(V+E)
/// instead of enumerating every simple path — on a dense, reconvergent (diamond)
/// call graph the naive "recurse into any non-stack node" version is exponential.
/// Any cycle in a strongly-connected component is still found, because a back
/// edge to the component's first-entered node is hit while that node is gray.
#[allow(clippy::too_many_arguments)]
fn find_cycles(
    node: u32,
    graph: &FxHashMap<u32, Vec<u32>>,
    stack: &mut Vec<u32>,
    on_stack: &mut FxHashSet<u32>,
    done: &mut FxHashSet<u32>,
    reported: &mut FxHashSet<Vec<u32>>,
    findings: &mut Vec<Finding>,
    ir: &Ir,
) {
    stack.push(node);
    on_stack.insert(node);

    if let Some(targets) = graph.get(&node) {
        for &next in targets {
            if on_stack.contains(&next) {
                // Cycle: slice of the stack from the first occurrence of `next`.
                let start = stack.iter().position(|&n| n == next).unwrap_or(0);
                let cycle: Vec<u32> = stack[start..].to_vec();
                let canon = canonical_cycle(&cycle);
                if reported.insert(canon.clone()) {
                    findings.push(Finding {
                        severity: Severity::Warning,
                        category: Category::DeadCode,
                        confidence: Confidence::Certain,
                        location: cycle_location(ir, cycle[0]),
                        message: Msg::CyclicCommonEvents { cycle },
                        references: Vec::new(),
                        rule: "cyclic-common-events",
                    });
                }
            } else if !done.contains(&next) {
                find_cycles(next, graph, stack, on_stack, done, reported, findings, ir);
            }
        }
    }

    stack.pop();
    on_stack.remove(&node);
    // Mark black: fully explored, never re-descend into it again.
    done.insert(node);
}

/// Canonical form of a cycle: rotated so that the smallest id comes first.
/// This makes the same cycle stably comparable regardless of the DFS root.
fn canonical_cycle(cycle: &[u32]) -> Vec<u32> {
    let Some(min_pos) = (0..cycle.len()).min_by_key(|&i| cycle[i]) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(cycle.len());
    out.extend_from_slice(&cycle[min_pos..]);
    out.extend_from_slice(&cycle[..min_pos]);
    out
}

/// Location of the common event by id (for the finding's primary location).
fn cycle_location(ir: &Ir, ce_id: u32) -> crate::ir::Location {
    ir.common_events_by_id
        .get(&ce_id)
        .and_then(|&e| ir.entity(e))
        .map(|n| n.location.clone())
        .unwrap_or_else(|| crate::ir::Location::file_only("data/CommonEvents.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{CeTrigger, CommonEvent, Engine, Ir, Location, PathSeg};

    fn push_ce(b: &mut crate::ir::IrBuilder, id: u32) -> EntityId {
        b.push_entity(
            Entity::CommonEvent(CommonEvent {
                id,
                name: format!("CE{id}"),
                trigger: CeTrigger::Parallel,
                command_count: 0,
            }),
            Location::new("data/CommonEvents.json", vec![PathSeg::CommonEvent(id)]),
        )
    }

    fn call(b: &mut crate::ir::IrBuilder, from: EntityId, to: u32) {
        b.push_edge(
            from,
            Edge::CallsCommonEvent {
                common_event_id: to,
            },
            Location::file_only("data/CommonEvents.json"),
        );
    }

    #[test]
    fn flags_cycle_and_spares_acyclic_chain() {
        let mut b = Ir::builder(Engine::Mz);
        // Cycle: 1 → 2 → 1.
        let ce1 = push_ce(&mut b, 1);
        let ce2 = push_ce(&mut b, 2);
        // Acyclic chain: 3 → 4 (control, not a cycle).
        let ce3 = push_ce(&mut b, 3);
        let _ce4 = push_ce(&mut b, 4);
        call(&mut b, ce1, 2);
        call(&mut b, ce2, 1);
        call(&mut b, ce3, 4);
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = CyclicCommonEvents.run(&ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "cyclic-common-events");
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(f[0].confidence, Confidence::Certain);
        let Msg::CyclicCommonEvents { cycle } = &f[0].message else {
            panic!("expected CyclicCommonEvents");
        };
        // Canonicalized: the smallest id (1) comes first.
        assert_eq!(cycle, &vec![1, 2]);
    }

    #[test]
    fn self_call_is_a_cycle() {
        let mut b = Ir::builder(Engine::Mz);
        let ce1 = push_ce(&mut b, 7);
        call(&mut b, ce1, 7); // 7 → 7
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);
        let f = CyclicCommonEvents.run(&ctx);
        assert_eq!(f.len(), 1);
        assert!(matches!(
            &f[0].message,
            Msg::CyclicCommonEvents { cycle } if cycle == &vec![7]
        ));
    }

    #[test]
    fn dense_reconvergent_dag_terminates_without_false_cycle() {
        // Diamond/reconvergent DAG (no cycle): a layered graph where each node
        // calls the whole next layer. The naive simple-path DFS would explore an
        // exponential number of paths here; the colored DFS must finish fast and
        // report nothing. `WIDTH^DEPTH` would be 4^12 ≈ 16.7M paths.
        const WIDTH: u32 = 4;
        const DEPTH: u32 = 12;
        let mut b = Ir::builder(Engine::Mz);
        let id = |layer: u32, i: u32| layer * WIDTH + i + 1;
        let mut ents = std::collections::HashMap::new();
        for layer in 0..=DEPTH {
            for i in 0..WIDTH {
                ents.insert(id(layer, i), push_ce(&mut b, id(layer, i)));
            }
        }
        for layer in 0..DEPTH {
            for i in 0..WIDTH {
                for j in 0..WIDTH {
                    call(&mut b, ents[&id(layer, i)], id(layer + 1, j));
                }
            }
        }
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);
        // Must terminate (no exponential blow-up) and find no cycle.
        assert!(CyclicCommonEvents.run(&ctx).is_empty());
    }
}
