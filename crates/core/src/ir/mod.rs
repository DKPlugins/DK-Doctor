//! Intermediate representation (IR) of the project — the engine-independent core.
//!
//! Submodules: [`location`] (finding addresses), [`entity`] (entities and arena),
//! [`edge`] (typed edges), [`symbols`] (switch/var table),
//! [`asset`] (asset references), [`graph`] (the [`Ir`] container and its builder).

pub mod asset;
pub mod dead_branch;
pub mod edge;
pub mod entity;
pub mod graph;
pub mod location;
pub mod plugin_meta;
pub mod self_switch;
pub mod symbols;

pub use asset::{AssetKey, AssetKind, AssetRef};
pub use dead_branch::{CmpOp, DeadBranch};
pub use edge::{Edge, EdgeRecord, TransferDesignation};
pub use entity::{
    CeTrigger, CommandMeta, CommonEvent, DatabaseRecord, DbKind, Entity, EntityId, EntityNode,
    Event, Map, Page, PageConditions, PageTrigger, PluginRef, ScriptBlackbox, Troop,
};
pub use graph::{Engine, Ir, IrBuilder, VehicleKind, VehicleStartMap};
pub use location::{Location, LocationPath, PathSeg};
pub use plugin_meta::{MethodPatch, PluginCommand, PluginCommandCall, PluginMeta, PluginOrderDeps};
pub use self_switch::{SelfSwitchInfo, SelfSwitchKey, SelfSwitchTable};
pub use symbols::{Site, SymbolInfo, SymbolTable};
