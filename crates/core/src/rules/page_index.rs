//! Auxiliary indexing of map event pages by `(map_id, event_id)`.
//!
//! [`crate::ir::Entity::Page`] entities are stored in the arena in load order: for
//! each event its pages go consecutively and in ascending order of number. The
//! `Event::page_ids`/`Map::event_ids` fields are not filled in by the adapter, so rules
//! that need the grouping of pages of a single event (`shadowed-page`,
//! `stuck-autorun`) reconstruct it from the logical path [`Location`]:
//! `[Map(map_id), Event(event_id), Page(page_no)]`.

use crate::ir::{Entity, EntityId, Ir, Page, PathSeg};

/// Snapshot of a single map event page: coordinates + references to entity/data.
pub struct PageRef<'a> {
    /// Id of the page entity in the arena.
    pub entity: EntityId,
    /// Event id (from the logical path).
    pub event_id: u32,
    /// Page number (1-based, from the logical path).
    pub page_no: u32,
    /// Page data.
    pub page: &'a Page,
}

/// Extracts `(map_id, event_id, page_no)` from the logical path of a map page.
///
/// Returns `None` for pages of common events/troops (they have no `Event`
/// segment) and for non-standard paths.
fn map_event_page(node: &crate::ir::EntityNode) -> Option<(u32, u32, u32)> {
    let segs = &node.location.path.0;
    match segs.as_slice() {
        [PathSeg::Map(m), PathSeg::Event(e), PathSeg::Page(p)] => Some((*m, *e, *p)),
        _ => None,
    }
}

/// Groups map event pages by `(map_id, event_id)`, preserving arena order
/// (== order of page numbers within an event).
///
/// Returns a vector of groups; each group is `((map_id, event_id), pages)`.
pub fn pages_by_event(ir: &Ir) -> Vec<((u32, u32), Vec<PageRef<'_>>)> {
    let mut groups: Vec<((u32, u32), Vec<PageRef<'_>>)> = Vec::new();
    for node in &ir.entities {
        let Entity::Page(page) = &node.kind else {
            continue;
        };
        let Some((map_id, event_id, page_no)) = map_event_page(node) else {
            continue;
        };
        let key = (map_id, event_id);
        let item = PageRef {
            entity: node.id,
            event_id,
            page_no,
            page,
        };
        match groups.last_mut() {
            Some((k, v)) if *k == key => v.push(item),
            _ => groups.push((key, vec![item])),
        }
    }
    groups
}
