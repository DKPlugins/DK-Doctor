//! Typed edges of the IR graph.
//!
//! Edges are stored as a flat `Vec` plus pointed indices in [`crate::ir::Ir`] —
//! rules need typed queries ("all Transfer edges", "all writes to
//! switch 14"), which a generic graph does not provide for free. Edge kinds are
//! engine-independent: "transfer", "call common event", "read/write symbol",
//! "asset reference", "DB record reference".

use crate::ir::asset::AssetKey;
use crate::ir::entity::{DbKind, EntityId};
use crate::ir::location::Location;

/// Edge record: source entity, the edge itself, and its location.
#[derive(Clone, Debug, serde::Serialize)]
pub struct EdgeRecord {
    /// Source entity of the edge.
    pub from: EntityId,
    /// Edge contents.
    pub edge: Edge,
    /// Location that produced the edge.
    pub location: Location,
}

/// Typed edge of the IR graph.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "edge")]
pub enum Edge {
    /// Transfer to a map (command 201). `to_map=None` — transfer by variable.
    Transfer {
        /// Target map (None — computed at runtime from a variable).
        to_map: Option<u32>,
        /// How the target is specified.
        designation: TransferDesignation,
    },
    /// Call a common event (command 117).
    CallsCommonEvent {
        /// Id of the called common event.
        common_event_id: u32,
    },
    /// Read a switch.
    ReadsSwitch {
        /// Switch id.
        switch_id: u32,
    },
    /// Write a switch.
    WritesSwitch {
        /// Switch id.
        switch_id: u32,
    },
    /// Read a variable.
    ReadsVariable {
        /// Variable id.
        variable_id: u32,
    },
    /// Write a variable.
    WritesVariable {
        /// Variable id.
        variable_id: u32,
    },
    /// Reference to an asset (image/sound/video).
    ReferencesAsset {
        /// Asset key.
        asset: AssetKey,
    },
    /// Reference to a database record by id.
    ReferencesDbId {
        /// DB record kind.
        kind: DbKind,
        /// Record id.
        id: u32,
    },
}

/// How a transfer target is specified (command 201, parameter `[0]`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransferDesignation {
    /// Direct map specification (`[0]==0`).
    Direct,
    /// Target computed from variables (`[0]==1`).
    ByVariable,
}
