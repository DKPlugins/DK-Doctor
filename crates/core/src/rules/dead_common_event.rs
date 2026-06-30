//! Rule `dead-common-event`: a common event that never runs.
//!
//! A common event with trigger [`CeTrigger::None`](crate::ir::CeTrigger::None)
//! runs only via an explicit call. If it has no incoming
//! call — neither an [`Edge::CallsCommonEvent`] edge (command 117), nor an
//! [`Edge::ReferencesDbId`] edge of kind [`DbKind::CommonEvent`] (effect 44) — it
//! is never executed (dead data code).
//!
//! Reservation from a script/plugin (`$gameTemp.reserveCommonEvent(N)` with a
//! literal id, Tier B) is accounted for: such events are spared via
//! [`Ir::reserved_common_events`](crate::ir::Ir). Only the
//! computed reservation ids remain unaccounted for — hence the `info` level (not an error).

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::{CeTrigger, DbKind, Edge, Entity};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};
use rustc_hash::FxHashSet;

/// Rule that finds unreachable common events (no trigger and no calls).
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

        // All common event ids that have an incoming call:
        // 117 (CallsCommonEvent) or effect 44 (ReferencesDbId{CommonEvent}).
        let mut called: FxHashSet<u32> = FxHashSet::default();
        for rec in &ir.edges {
            match rec.edge {
                Edge::CallsCommonEvent { common_event_id } => {
                    called.insert(common_event_id);
                }
                Edge::ReferencesDbId {
                    kind: DbKind::CommonEvent,
                    id,
                } => {
                    called.insert(id);
                }
                _ => {}
            }
        }

        let mut findings = Vec::new();
        for node in &ir.entities {
            if let Entity::CommonEvent(ce) = &node.kind
                && ce.trigger == CeTrigger::None
                && !called.contains(&ce.id)
                && !ir.reserved_common_events.contains(&ce.id)
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
    use crate::ir::{CommonEvent, Engine, EntityId, Ir, Location, PathSeg};

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

    #[test]
    fn flags_uncalled_none_trigger_event_only() {
        let mut b = Ir::builder(Engine::Mz);
        // #1: None, no calls → dead.
        let _dead = push_ce(&mut b, 1, CeTrigger::None);
        // #2: None, but called via 117 from #3 → fine (control).
        let _called = push_ce(&mut b, 2, CeTrigger::None);
        // #3: Parallel → runs on its trigger, and calls #2.
        let caller = push_ce(&mut b, 3, CeTrigger::Parallel);
        b.push_edge(
            caller,
            Edge::CallsCommonEvent { common_event_id: 2 },
            Location::file_only("data/CommonEvents.json"),
        );
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
        use crate::ir::{PathSeg, PluginRef};
        let mut b = Ir::builder(Engine::Mz);
        let dead = push_ce(&mut b, 7, CeTrigger::None);
        // The plugin references CE #7 via a @type common_event parameter →
        // reserved by the plugin, not dead. from = Entity::Plugin (the kind of
        // the edge source does not matter — the rule looks only at the id).
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
        // Effect 44 references #4 → runs via an item/skill.
        b.push_edge(
            dead,
            Edge::ReferencesDbId {
                kind: DbKind::CommonEvent,
                id: 4,
            },
            Location::file_only("data/Items.json"),
        );
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);
        assert!(DeadCommonEvent.run(&ctx).is_empty());
    }
}
