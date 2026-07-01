//! Rule `dead-common-event`: a common event that never runs.
//!
//! A common event with trigger [`CeTrigger::None`](crate::ir::CeTrigger::None)
//! runs only via an explicit call. It is dead unless it is **reachable** from a
//! live entry point: a triggered (Autorun/Parallel) event, a `117` call or an
//! effect-44 database reference originating outside the common-event call graph,
//! or a reservation. Reachability is transitive over the CommonEvent→CommonEvent
//! call graph (see [`CommonEventSummary`](crate::ir::CommonEventSummary)), so a
//! cluster of common events that only call **each other** — with no live caller —
//! is correctly flagged, a false-negative the plain "has any incoming 117" check
//! misses.
//!
//! Reservation from a script/plugin (`$gameTemp.reserveCommonEvent(N)` with a
//! literal id, Tier B) is accounted for: such events are reachability roots via
//! [`Ir::reserved_common_events`](crate::ir::Ir). Only the computed reservation
//! ids remain unaccounted for — hence the `info` level (not an error).

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::{CeTrigger, Entity};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};

/// Rule that finds unreachable common events (never run).
pub struct DeadCommonEvent;

impl Rule for DeadCommonEvent {
    fn id(&self) -> &'static str {
        "dead-common-event"
    }

    fn category(&self) -> Category {
        Category::Data
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let ir = ctx.ir;

        let mut findings = Vec::new();
        for node in &ir.entities {
            if let Entity::CommonEvent(ce) = &node.kind
                && ce.trigger == CeTrigger::None
                // Transitive reachability from a live entry point. A reserved event
                // is a reachability root, so it is covered here too.
                && !ir
                    .common_event_summaries
                    .get(&ce.id)
                    .is_some_and(|s| s.reachable)
            {
                findings.push(Finding {
                    severity: Severity::Info,
                    category: Category::Data,
                    confidence: Confidence::Certain,
                    location: node.location.clone(),
                    message: Msg::DeadCommonEvent {
                        id: ce.id,
                        name: ce.name.clone(),
                    },
                    references: Vec::new(),
                    rule: "dead-common-event",
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
        CommonEvent, DatabaseRecord, DbKind, Edge, Engine, EntityId, Ir, Location, PathSeg,
    };

    fn push_ce(b: &mut crate::ir::IrBuilder, id: u32, trigger: CeTrigger) -> EntityId {
        b.push_entity(
            Entity::CommonEvent(CommonEvent {
                id,
                name: format!("CE{id}"),
                trigger,
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
    fn flags_uncalled_none_trigger_event_only() {
        let mut b = Ir::builder(Engine::Mz);
        // #1: None, no calls → dead.
        let _dead = push_ce(&mut b, 1, CeTrigger::None);
        // #2: None, but called via 117 from #3 → fine (control).
        let _called = push_ce(&mut b, 2, CeTrigger::None);
        // #3: Parallel → runs on its trigger, and calls #2.
        let caller = push_ce(&mut b, 3, CeTrigger::Parallel);
        call(&mut b, caller, 2);
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = DeadCommonEvent.run(&ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "dead-common-event");
        assert_eq!(f[0].severity, Severity::Info);
        assert_eq!(f[0].confidence, Confidence::Certain);
        assert!(matches!(f[0].message, Msg::DeadCommonEvent { id: 1, .. }));
    }

    #[test]
    fn plugin_common_event_ref_spares_event() {
        use crate::ir::PluginRef;
        let mut b = Ir::builder(Engine::Mz);
        let dead = push_ce(&mut b, 7, CeTrigger::None);
        // The plugin references CE #7 via a @type common_event parameter → a live
        // entry point from outside the call graph, so #7 is reachable (not dead).
        let plugin = b.push_entity(
            Entity::Plugin(PluginRef {
                name: "Loc".to_string(),
            }),
            Location::new("js/plugins.js", vec![PathSeg::Plugin("Loc".to_string())]),
        );
        b.push_edge(
            plugin,
            Edge::ReferencesDbId {
                kind: DbKind::CommonEvent,
                id: 7,
            },
            Location::new(
                "js/plugins.js",
                vec![
                    PathSeg::Plugin("Loc".to_string()),
                    PathSeg::Param("OnLangChange".to_string()),
                ],
            ),
        );
        let _ = dead;
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);
        assert!(DeadCommonEvent.run(&ctx).is_empty());
    }

    #[test]
    fn reserved_common_event_spares_event() {
        let mut b = Ir::builder(Engine::Mz);
        // #8: None, no 117/effect 44, but reserved by a script → not dead.
        let _ = push_ce(&mut b, 8, CeTrigger::None);
        b.add_reserved_common_event(8);
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);
        assert!(DeadCommonEvent.run(&ctx).is_empty());
    }

    #[test]
    fn effect_44_reference_spares_event() {
        let mut b = Ir::builder(Engine::Mz);
        let dead = push_ce(&mut b, 4, CeTrigger::None);
        // Effect 44: an Item record references CE #4 → runs via the item. The edge
        // originates from a DatabaseRecord (outside the call graph) → live root.
        let item = b.push_entity(
            Entity::DatabaseRecord(DatabaseRecord {
                kind: DbKind::Item,
                record_id: 1,
                name: "Elixir".to_string(),
            }),
            Location::file_only("data/Items.json"),
        );
        b.push_edge(
            item,
            Edge::ReferencesDbId {
                kind: DbKind::CommonEvent,
                id: 4,
            },
            Location::file_only("data/Items.json"),
        );
        let _ = dead;
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);
        assert!(DeadCommonEvent.run(&ctx).is_empty());
    }

    #[test]
    fn flags_mutually_calling_dead_cluster() {
        // #3 ↔ #4 call each other but nothing live ever enters the cluster → both
        // are dead (the plain "has an incoming 117" check misses this).
        let mut b = Ir::builder(Engine::Mz);
        let a = push_ce(&mut b, 3, CeTrigger::None);
        let c = push_ce(&mut b, 4, CeTrigger::None);
        call(&mut b, a, 4);
        call(&mut b, c, 3);
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);
        let f = DeadCommonEvent.run(&ctx);
        let ids: Vec<u32> = f
            .iter()
            .filter_map(|f| match f.message {
                Msg::DeadCommonEvent { id, .. } => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(ids, vec![3, 4], "both members of the dead cluster flagged");
    }

    #[test]
    fn reachable_cycle_is_not_dead() {
        // Same #3 ↔ #4 cycle, but now a triggered #5 enters it → both reachable,
        // neither is dead (they would still be flagged by cyclic-common-events).
        let mut b = Ir::builder(Engine::Mz);
        let a = push_ce(&mut b, 3, CeTrigger::None);
        let c = push_ce(&mut b, 4, CeTrigger::None);
        let live = push_ce(&mut b, 5, CeTrigger::Parallel);
        call(&mut b, a, 4);
        call(&mut b, c, 3);
        call(&mut b, live, 3);
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);
        assert!(
            DeadCommonEvent.run(&ctx).is_empty(),
            "a cycle reached from a triggered event is not dead"
        );
    }
}
