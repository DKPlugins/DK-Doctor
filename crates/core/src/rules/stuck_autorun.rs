//! Rule `stuck-autorun`: an **Autorun** page cannot turn itself
//! off — the game hangs (soft-lock).
//!
//! Autorun blocks input while active; if such a page is enabled by a condition and
//! does nothing to clear that condition, the game freezes. To terminate
//! correctly, the page usually: writes a self-switch (123 → `Ir.self_switches`),
//! writes a global switch ([`Edge::WritesSwitch`]) or transfers the player
//! ([`Edge::Transfer`]).
//!
//! **Autorun only, not Parallel.** Parallel does not block input; an "eternal" Parallel
//! in the corpus is almost always an intentional background process (HUD, input polling, no-op),
//! so flagging it = noise (on dev projects the stuck-autorun remainder was ~all
//! parallel). A busy-loop Parallel is intentionally not flagged by this rule.
//!
//! Conservatism (to avoid flooding a plugin/script-heavy corpus):
//! - we flag only **gated** pages (an Autorun without conditions is most often a cutscene);
//! - a self-switch write by the event — a frequent legitimate "exit" — suppresses the trigger;
//! - a page with a plugin command / script (`opaque_exit_codes`, 355/356/357) may
//!   exit untraceably — we don't flag it;
//! - a page whose **common-event call** (117) could hide an exit — its callee (or
//!   anything it transitively calls) writes a switch/variable, transfers, or is
//!   opaque ([`CommonEventSummary::provides_exit`](crate::ir::CommonEventSummary)) —
//!   is spared. A `117` to a **proven cosmetic** common event (text/pictures/sound
//!   only) no longer hides the exit, so such a page is still flagged: that is the
//!   interprocedural refinement over the old blunt "any `117` suppresses";
//! - a gating-switch that is set to OFF somewhere (`ever_set_off`), is never
//!   written (never ON) or is managed by a plugin (`declared_by_plugin`/
//!   `set_by_plugin`, Tier A/B) — the page is clearable/unactivatable, we don't flag it;
//! - confidence `likely`: the exit may hide in a computed script/plugin.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::{Edge, EntityId, PageTrigger};
use crate::message::Msg;
use crate::rules::page_index::pages_by_event;
use crate::rules::{Rule, RuleCtx};
use rustc_hash::FxHashSet;

/// Rule for finding non-terminating Autorun/Parallel pages.
pub struct StuckAutorun;

/// Whether a page's gating condition can be CLEARED, or the page never
/// activates at all (in both cases this is not a guaranteed soft-lock). Page
/// conditions are combined with AND, so it's enough that AT LEAST ONE holds:
///  - a gating-**switch** is "clearable" if: it is set to OFF somewhere
///    (`ever_set_off`, usually by another event — F&H2 flood fix); is managed by
///    a plugin (`declared_by_plugin`/`set_by_plugin`, Tier A/B); OR is never
///    written (`writes.is_empty()`) — then it is never turned on, the page does not
///    activate and does not loop (this is the `uninitialized-symbols` case).
///    A real soft-lock remains only with a switch that is set to ON and
///    never turned off;
///  - a gating-**variable** — if managed by a plugin (a variable has no
///    unambiguous "turn-off", so only the plugin signal counts).
///
/// A self-switch condition is not considered here: its clearing is tracked
/// separately via a self-switch write on the same page
/// (`pages_with_self_switch_write`).
fn gating_condition_clearable(ctx: &RuleCtx<'_>, cond: &crate::ir::PageConditions) -> bool {
    let switch_clearable = |id: u32| match ctx.ir.symbols.switches.get(&id) {
        // No entry in the table ⇒ the switch is never written ⇒ never ON ⇒
        // the page does not activate (uninitialized-symbols case), not a soft-lock.
        None => true,
        Some(s) => s.declared_by_plugin || s.set_by_plugin || s.ever_set_off || s.writes.is_empty(),
    };
    let var_owned = |id: u32| {
        ctx.ir
            .symbols
            .variables
            .get(&id)
            .is_some_and(|s| s.declared_by_plugin || s.set_by_plugin)
    };
    cond.switch1.is_some_and(switch_clearable)
        || cond.switch2.is_some_and(switch_clearable)
        || cond.variable.is_some_and(var_owned)
}

impl Rule for StuckAutorun {
    fn id(&self) -> &'static str {
        "stuck-autorun"
    }

    fn category(&self) -> Category {
        Category::Reference
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        // Precomputations (one pass each) — otherwise the rule was quadratic on
        // large projects: the self-switch check ran over the whole table for EVERY
        // event, and the presence of an "exit" — over all edges for EVERY page.
        // (1) page entities that write a self-switch.  A self-switch write only
        // proves that the currently analyzed autorun page can turn itself off if
        // the write is reachable from that same page.  Another page of the same
        // event can be selected under different conditions (or shadow the autorun)
        // and must not suppress a stuck-autorun finding for this page.
        let pages_with_self_switch_write: FxHashSet<EntityId> = ctx
            .ir
            .self_switches
            .entries
            .values()
            .flat_map(|info| info.writes.iter().map(|site| site.entity))
            .collect();
        // (2) page entities with an "exit" edge (switch write / player transfer).
        let pages_with_exit: FxHashSet<EntityId> = ctx
            .ir
            .edges
            .iter()
            .filter(|r| matches!(r.edge, Edge::WritesSwitch { .. } | Edge::Transfer { .. }))
            .map(|r| r.from)
            .collect();
        // (3) page entities that call a common event which could hide an exit:
        // the callee (transitively) provides an exit, or its summary is unknown.
        // A `117` to a PROVEN cosmetic common event (provides_exit == false) is
        // intentionally NOT counted here, so such a page can still be flagged.
        let pages_calling_opaque_ce: FxHashSet<EntityId> = ctx
            .ir
            .edges
            .iter()
            .filter_map(|r| match r.edge {
                Edge::CallsCommonEvent { common_event_id } => {
                    let hides_exit = ctx
                        .ir
                        .common_event_summaries
                        .get(&common_event_id)
                        .is_none_or(|s| s.provides_exit);
                    hides_exit.then_some(r.from)
                }
                _ => None,
            })
            .collect();

        let mut findings = Vec::new();
        for (_, pages) in pages_by_event(ctx.ir) {
            for page in &pages {
                // Autorun only: it blocks input → a non-terminating Autorun
                // hangs the game (soft-lock) = a real bug. A Parallel page
                // that "spins forever" in the corpus is almost always an intentional
                // background process (HUD/input/no-op) — flagging it = noise, so
                // we do NOT flag it (data-driven: on dev projects the remainder was ~all
                // parallel). A busy-loop Parallel is intentionally out of scope for this
                // static rule at `warning`.
                if page.page.trigger != PageTrigger::Autorun {
                    continue;
                }
                // Only gated pages (there is a controlling condition).
                let cond = &page.page.conditions;
                let gated = cond.switch1.is_some()
                    || cond.switch2.is_some()
                    || cond.variable.is_some()
                    || cond.self_switch.is_some();
                if !gated {
                    continue;
                }
                if pages_with_self_switch_write.contains(&page.entity) {
                    continue;
                }
                // The page runs a plugin command / script (`opaque_exit_codes`,
                // 355/356/357) ⇒ the exit may hide in code static analysis cannot
                // trace. We don't flag it (flood fix on plugin/script-heavy: F&H2,
                // Heroines).
                if page
                    .page
                    .commands
                    .iter()
                    .any(|c| ctx.opaque_exit_codes.contains(&c.code))
                {
                    continue;
                }
                // The page calls a common event that could hide an exit (its
                // callee provides an exit transitively, or is unknown). A `117` to
                // a proven-cosmetic common event does NOT suppress here.
                if pages_calling_opaque_ce.contains(&page.entity) {
                    continue;
                }
                // If the gating condition can be cleared (the switch is set to OFF
                // by another event / the symbol is managed by a plugin) — this is not
                // a guaranteed soft-lock. We don't flag it (F&H2 flood fix).
                if gating_condition_clearable(ctx, cond) {
                    continue;
                }
                // Does the page write a switch / transfer the player?
                if pages_with_exit.contains(&page.entity) {
                    continue;
                }
                let location = ctx
                    .ir
                    .entity(page.entity)
                    .map(|n| n.location.clone())
                    .unwrap_or_else(|| crate::ir::Location::file_only(""));
                findings.push(Finding {
                    severity: Severity::Warning,
                    category: Category::Reference,
                    confidence: Confidence::Likely,
                    location,
                    message: Msg::StuckAutorun {
                        page: page.page_no,
                        event: page.event_id,
                    },
                    references: Vec::new(),
                    rule: "stuck-autorun",
                });
            }
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        Engine, Entity, EntityId, Ir, IrBuilder, Location, Page, PageConditions, PageTrigger,
        PathSeg, SelfSwitchKey, Site,
    };

    fn push_page(
        b: &mut IrBuilder,
        map: u32,
        event: u32,
        no: u32,
        trigger: PageTrigger,
        cond: PageConditions,
    ) -> EntityId {
        let base = vec![PathSeg::Map(map), PathSeg::Event(event), PathSeg::Page(no)];
        b.push_entity(
            Entity::Page(Page {
                conditions: cond,
                trigger,
                command_count: 0,
                commands: vec![],
            }),
            Location::new("data/Map001.json", base),
        )
    }

    fn gated() -> PageConditions {
        PageConditions {
            self_switch: Some('A'),
            ..Default::default()
        }
    }

    #[test]
    fn flags_gated_autorun_with_no_exit() {
        let mut b = Ir::builder(Engine::Mz);
        push_page(&mut b, 1, 5, 1, PageTrigger::Autorun, gated());
        let ir = b.finish();
        let f = StuckAutorun.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "stuck-autorun");
        assert_eq!(f[0].severity, Severity::Warning);
        assert!(matches!(
            f[0].message,
            Msg::StuckAutorun { page: 1, event: 5 }
        ));
    }

    #[test]
    fn spares_parallel_pages() {
        // Parallel is not flagged (intentional background processes) — even without an exit.
        let mut b = Ir::builder(Engine::Mz);
        push_page(&mut b, 1, 5, 1, PageTrigger::Parallel, gated());
        let ir = b.finish();
        assert!(StuckAutorun.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn spares_autorun_that_writes_self_switch() {
        let mut b = Ir::builder(Engine::Mz);
        let pe = push_page(&mut b, 1, 5, 1, PageTrigger::Autorun, gated());
        // The autorun page writes self-switch 'A' → an exit is possible.
        b.add_self_switch_write(
            SelfSwitchKey::new(1, 5, 'A'),
            Site {
                location: Location::file_only("data/Map001.json"),
                entity: pe,
            },
        );
        let ir = b.finish();
        assert!(StuckAutorun.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn self_switch_write_on_another_page_does_not_suppress_autorun() {
        let mut b = Ir::builder(Engine::Mz);
        let autorun = push_page(&mut b, 1, 5, 1, PageTrigger::Autorun, gated());
        let action = push_page(
            &mut b,
            1,
            5,
            2,
            PageTrigger::Action,
            PageConditions::default(),
        );
        // A different page of the same event can write the self-switch, but that
        // does not make the autorun page terminate.
        b.add_self_switch_write(
            SelfSwitchKey::new(1, 5, 'A'),
            Site {
                location: Location::file_only("data/Map001.json"),
                entity: action,
            },
        );
        let ir = b.finish();
        let f = StuckAutorun.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert!(matches!(
            f[0].message,
            Msg::StuckAutorun { page: 1, event: 5 }
        ));
        assert_ne!(autorun, action);
    }

    #[test]
    fn spares_autorun_that_writes_switch() {
        let mut b = Ir::builder(Engine::Mz);
        let pe = push_page(&mut b, 1, 5, 1, PageTrigger::Autorun, gated());
        b.push_edge(
            pe,
            Edge::WritesSwitch { switch_id: 7 },
            Location::file_only("data/Map001.json"),
        );
        let ir = b.finish();
        assert!(StuckAutorun.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn spares_ungated_autorun_and_action_pages() {
        let mut b = Ir::builder(Engine::Mz);
        // Autorun without conditions (probably a cutscene) — not flagged.
        push_page(
            &mut b,
            1,
            5,
            1,
            PageTrigger::Autorun,
            PageConditions::default(),
        );
        // An Action page with a condition — not Autorun/Parallel, not flagged.
        push_page(&mut b, 1, 6, 1, PageTrigger::Action, gated());
        let ir = b.finish();
        assert!(StuckAutorun.run(&RuleCtx::new(&ir)).is_empty());
    }

    fn gated_switch(id: u32) -> PageConditions {
        PageConditions {
            switch1: Some(id),
            ..Default::default()
        }
    }

    #[test]
    fn suppresses_page_gated_on_plugin_owned_switch() {
        // The page is enabled by switch #42 owned by a plugin ⇒ not flagged.
        let mut b = Ir::builder(Engine::Mz);
        push_page(&mut b, 1, 5, 1, PageTrigger::Autorun, gated_switch(42));
        b.symbols_mut().mark_switch_declared_by_plugin(42);
        let ir = b.finish();
        assert!(StuckAutorun.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn still_flags_page_gated_on_switch_set_on_never_off() {
        // switch #42 is set to ON (there is a write) and is never turned off, not
        // plugin-managed ⇒ a real soft-lock, we flag it.
        let mut b = Ir::builder(Engine::Mz);
        let pe = push_page(&mut b, 1, 5, 1, PageTrigger::Autorun, gated_switch(42));
        b.symbols_mut().add_switch_write(
            42,
            Site {
                location: Location::file_only("data/Map002.json"),
                entity: pe,
            },
        );
        let ir = b.finish();
        assert_eq!(StuckAutorun.run(&RuleCtx::new(&ir)).len(), 1);
    }

    #[test]
    fn suppresses_page_gated_on_never_written_switch() {
        // switch #42 is never written ⇒ never ON ⇒ the page does not activate
        // (uninitialized-symbols case), not a soft-lock. Not flagged.
        let mut b = Ir::builder(Engine::Mz);
        push_page(&mut b, 1, 5, 1, PageTrigger::Autorun, gated_switch(42));
        let ir = b.finish();
        assert!(StuckAutorun.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn suppresses_page_gated_on_switch_set_by_plugin_js() {
        // Tier B: switch #42 is written by plugin JS code ($gameSwitches.setValue) ⇒
        // the plugin clears the page. Not flagged (the main F&H2 flood fix).
        let mut b = Ir::builder(Engine::Mz);
        push_page(&mut b, 1, 5, 1, PageTrigger::Autorun, gated_switch(42));
        b.symbols_mut().mark_switch_set_by_plugin(42);
        let ir = b.finish();
        assert!(StuckAutorun.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn suppresses_page_gated_on_switch_ever_set_off() {
        // Gating-switch #42 is set to OFF somewhere (by another event) ⇒ the page
        // can be cleared, not a guaranteed soft-lock. Not flagged (F&H2 fix).
        let mut b = Ir::builder(Engine::Mz);
        push_page(&mut b, 1, 5, 1, PageTrigger::Autorun, gated_switch(42));
        b.symbols_mut().mark_switch_ever_set_off(42);
        let ir = b.finish();
        assert!(StuckAutorun.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn still_flags_switch_only_ever_set_on() {
        // switch #42 is written only to ON (ever_set_off=false) ⇒ cannot be cleared ⇒
        // a real soft-lock, we flag it.
        let mut b = Ir::builder(Engine::Mz);
        let pe = push_page(&mut b, 1, 5, 1, PageTrigger::Autorun, gated_switch(42));
        // A switch #42 write EXISTS (as a site), but only ON — we don't set ever_set_off.
        b.symbols_mut().add_switch_write(
            42,
            Site {
                location: Location::file_only("data/Map002.json"),
                entity: pe,
            },
        );
        let ir = b.finish();
        assert_eq!(StuckAutorun.run(&RuleCtx::new(&ir)).len(), 1);
    }

    #[test]
    fn suppresses_page_gated_on_variable_set_by_plugin_js() {
        // Tier B: the page is enabled by variable #9, which is written by plugin JS.
        let mut b = Ir::builder(Engine::Mz);
        let cond = PageConditions {
            variable: Some(9),
            variable_value: Some(1),
            ..Default::default()
        };
        push_page(&mut b, 1, 5, 1, PageTrigger::Autorun, cond);
        b.symbols_mut().mark_variable_set_by_plugin(9);
        let ir = b.finish();
        assert!(StuckAutorun.run(&RuleCtx::new(&ir)).is_empty());
    }

    /// Pushes a common event entity with the given id/trigger (helper for the
    /// 117 look-through tests).
    fn push_ce(b: &mut IrBuilder, id: u32, trigger: crate::ir::CeTrigger) -> EntityId {
        b.push_entity(
            Entity::CommonEvent(crate::ir::CommonEvent {
                id,
                name: format!("CE{id}"),
                trigger,
                command_count: 0,
            }),
            Location::new("data/CommonEvents.json", vec![PathSeg::CommonEvent(id)]),
        )
    }

    #[test]
    fn spares_autorun_calling_exit_providing_common_event() {
        // Autorun gated on self-switch 'A', no direct exit, but it calls CE #10
        // which writes a switch → the callee provides an exit → not flagged.
        use crate::ir::CeTrigger;
        let mut b = Ir::builder(Engine::Mz);
        let base = vec![PathSeg::Map(1), PathSeg::Event(5), PathSeg::Page(1)];
        let page = b.push_entity(
            Entity::Page(Page {
                conditions: gated(),
                trigger: PageTrigger::Autorun,
                command_count: 1,
                commands: vec![crate::ir::CommandMeta {
                    code: 117,
                    indent: 0,
                    index: 0,
                    location: Location::file_only("data/Map001.json"),
                }],
            }),
            Location::new("data/Map001.json", base),
        );
        b.push_edge(
            page,
            Edge::CallsCommonEvent {
                common_event_id: 10,
            },
            Location::file_only("data/Map001.json"),
        );
        // CE #10 writes a switch → provides_exit.
        let ce = push_ce(&mut b, 10, CeTrigger::None);
        b.push_edge(
            ce,
            Edge::WritesSwitch { switch_id: 7 },
            Location::file_only("data/CommonEvents.json"),
        );
        let ir = b.finish();
        let ctx = RuleCtx::with_codes(&ir, &[], &[355, 356, 357], &[]);
        assert!(StuckAutorun.run(&ctx).is_empty());
    }

    #[test]
    fn flags_autorun_calling_cosmetic_common_event() {
        // Same page, but CE #10 is purely cosmetic (no switch/var write, no
        // transfer, not opaque) → its call cannot hide an exit → still a soft-lock.
        use crate::ir::CeTrigger;
        let mut b = Ir::builder(Engine::Mz);
        let base = vec![PathSeg::Map(1), PathSeg::Event(5), PathSeg::Page(1)];
        let page = b.push_entity(
            Entity::Page(Page {
                conditions: gated(),
                trigger: PageTrigger::Autorun,
                command_count: 1,
                commands: vec![crate::ir::CommandMeta {
                    code: 117,
                    indent: 0,
                    index: 0,
                    location: Location::file_only("data/Map001.json"),
                }],
            }),
            Location::new("data/Map001.json", base),
        );
        b.push_edge(
            page,
            Edge::CallsCommonEvent {
                common_event_id: 10,
            },
            Location::file_only("data/Map001.json"),
        );
        // CE #10 has no edges → provides_exit == false (proven cosmetic).
        let _ = push_ce(&mut b, 10, CeTrigger::None);
        let ir = b.finish();
        let ctx = RuleCtx::with_codes(&ir, &[], &[355, 356, 357], &[]);
        let f = StuckAutorun.run(&ctx);
        assert_eq!(f.len(), 1);
        assert!(matches!(
            f[0].message,
            Msg::StuckAutorun { page: 1, event: 5 }
        ));
    }

    #[test]
    fn suppresses_page_with_opaque_exit_command() {
        // The page calls a common event (117 in opaque_exit_codes) ⇒ the exit may
        // hide there, not flagged.
        let mut b = Ir::builder(Engine::Mz);
        let base = vec![PathSeg::Map(1), PathSeg::Event(5), PathSeg::Page(1)];
        b.push_entity(
            Entity::Page(Page {
                conditions: gated(),
                trigger: PageTrigger::Autorun,
                command_count: 1,
                commands: vec![crate::ir::CommandMeta {
                    code: 117,
                    indent: 0,
                    index: 0,
                    location: Location::file_only("data/Map001.json"),
                }],
            }),
            Location::new("data/Map001.json", base),
        );
        let ir = b.finish();
        let ctx = RuleCtx::with_codes(&ir, &[], &[117, 355, 356, 357], &[]);
        assert!(StuckAutorun.run(&ctx).is_empty());
        // Without opaque codes (empty context) the same page is flagged.
        assert_eq!(StuckAutorun.run(&RuleCtx::new(&ir)).len(), 1);
    }
}
