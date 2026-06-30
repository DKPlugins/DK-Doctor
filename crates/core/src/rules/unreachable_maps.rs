//! `unreachable-maps` rule: maps unreachable via direct transfers.
//!
//! BFS from `start_map_id` along [`Edge::Transfer`] edges with a `Direct`
//! designation (the target is a concrete map). A map not reached by this
//! traversal and that is not the start map is flagged as unreachable. Static
//! confidence is `certain`, but the level is lowered to `info`: the map may
//! still be opened by a transfer by variable, by a plugin, or by a common
//! event (decision on open Q #3).

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::edge::TransferDesignation;
use crate::ir::{Edge, Entity, EntityId};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};
use rustc_hash::{FxHashMap, FxHashSet};

/// Rule that finds unreachable maps.
pub struct UnreachableMaps;

impl Rule for UnreachableMaps {
    fn id(&self) -> &'static str {
        "unreachable-maps"
    }

    fn category(&self) -> Category {
        Category::Reference
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let ir = ctx.ir;
        let Some(start) = ir.start_map_id else {
            // Without a start map the notion of reachability is undefined.
            return Vec::new();
        };

        // Direct-transfer graph: map_id → set of target map_id.
        // The edge is bound to the source entity; we find the event's owning map.
        let mut adjacency: FxHashMap<u32, Vec<u32>> = FxHashMap::default();
        for rec in &ir.edges {
            if let Edge::Transfer {
                to_map: Some(target),
                designation: TransferDesignation::Direct,
            } = rec.edge
                && let Some(src_map) = owning_map_id(ir, rec.from)
            {
                adjacency.entry(src_map).or_default().push(target);
            }
        }

        // BFS from the start map.
        let mut reachable: FxHashSet<u32> = FxHashSet::default();
        reachable.insert(start);
        let mut queue = vec![start];
        while let Some(map_id) = queue.pop() {
            if let Some(targets) = adjacency.get(&map_id) {
                for &t in targets {
                    if reachable.insert(t) {
                        queue.push(t);
                    }
                }
            }
        }

        // Maps not present in reachable. Maps declared by a plugin (profile —
        // e.g. DK_Event_Factory template-event maps) are excluded: the player
        // does not visit them, the plugin uses them as a source.
        let mut findings = Vec::new();
        for node in &ir.entities {
            if let Entity::Map(m) = &node.kind
                && !reachable.contains(&m.map_id)
                && !ir.plugin_referenced_maps.contains(&m.map_id)
            {
                findings.push(Finding {
                    severity: Severity::Info,
                    category: Category::Reference,
                    confidence: Confidence::Certain,
                    location: node.location.clone(),
                    message: Msg::UnreachableMap {
                        map_id: m.map_id,
                        name: m.name.clone(),
                    },
                    references: Vec::new(),
                    rule: "unreachable-maps",
                });
            }
        }
        findings
    }
}

/// Finds the owning map id for the edge's source entity.
///
/// The source of a `Transfer` may be an event page (whose owner is the map via
/// `Event::map_id`) or the map itself; common events/troops have no map.
fn owning_map_id(ir: &crate::ir::Ir, from: EntityId) -> Option<u32> {
    match &ir.entity(from)?.kind {
        Entity::Map(m) => Some(m.map_id),
        Entity::Event(e) => Some(e.map_id),
        // A page does not store map_id directly; the adapter attaches the
        // page's Transfer edges to the page entity. We recover the map via its
        // location.
        Entity::Page(_) => page_map_id(ir, from),
        _ => None,
    }
}

/// Recovers a page's map_id from the first segment of its logical path.
fn page_map_id(ir: &crate::ir::Ir, from: EntityId) -> Option<u32> {
    use crate::ir::PathSeg;
    let node = ir.entity(from)?;
    node.location.path.0.iter().find_map(|seg| match seg {
        PathSeg::Map(id) => Some(*id),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Engine, Ir, Location, Map, PathSeg};

    fn push_map(b: &mut crate::ir::IrBuilder, id: u32) -> EntityId {
        b.push_entity(
            Entity::Map(Map {
                map_id: id,
                name: format!("Map{id}"),
                event_ids: vec![],
                can_battle: false,
            }),
            Location::new(format!("data/Map{id:03}.json"), vec![PathSeg::Map(id)]),
        )
    }

    #[test]
    fn flags_only_island_map() {
        let mut b = Ir::builder(Engine::Mz);
        let m1 = push_map(&mut b, 1); // start
        let _m2 = push_map(&mut b, 2); // reachable from 1
        let _m3 = push_map(&mut b, 3); // island (no incoming direct transfer)
        b.set_start_map(Some(1));
        // 1 → 2 direct transfer.
        b.push_edge(
            m1,
            Edge::Transfer {
                to_map: Some(2),
                designation: TransferDesignation::Direct,
            },
            Location::new("data/Map001.json", vec![PathSeg::Map(1)]),
        );
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = UnreachableMaps.run(&ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Info);
        assert!(matches!(
            f[0].message,
            Msg::UnreachableMap { map_id: 3, .. }
        ));
    }

    #[test]
    fn plugin_referenced_map_is_not_flagged() {
        let mut b = Ir::builder(Engine::Mz);
        let _m1 = push_map(&mut b, 1); // start
        let _m2 = push_map(&mut b, 2); // island, but referenced by a plugin
        let _m3 = push_map(&mut b, 3); // island, genuinely unreachable
        b.set_start_map(Some(1));
        b.add_plugin_referenced_map(2); // e.g. DK_Event_Factory template map
        let ir = b.finish();
        let f = UnreachableMaps.run(&RuleCtx::new(&ir));
        // Only map 3; map 2 is suppressed by the plugin declaration.
        assert_eq!(f.len(), 1);
        assert!(matches!(
            f[0].message,
            Msg::UnreachableMap { map_id: 3, .. }
        ));
    }

    #[test]
    fn by_variable_does_not_make_map_reachable() {
        let mut b = Ir::builder(Engine::Mz);
        let m1 = push_map(&mut b, 1);
        let _m4 = push_map(&mut b, 4);
        b.set_start_map(Some(1));
        // 1 → 4, but by variable → not counted as reachable.
        b.push_edge(
            m1,
            Edge::Transfer {
                to_map: None,
                designation: TransferDesignation::ByVariable,
            },
            Location::new("data/Map001.json", vec![PathSeg::Map(1)]),
        );
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);
        let f = UnreachableMaps.run(&ctx);
        assert_eq!(f.len(), 1);
        assert!(matches!(
            f[0].message,
            Msg::UnreachableMap { map_id: 4, .. }
        ));
    }
}
