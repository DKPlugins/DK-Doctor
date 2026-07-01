//! Root IR container ([`Ir`]) and its construction.
//!
//! The IR is built once and queried many times. Entities are an arena `Vec`,
//! edges are a flat `Vec` plus targeted indices (`maps_by_id`, `db`, …)
//! that are finalized after population. The adapter uses [`IrBuilder`].

use crate::ir::asset::AssetKey;
use crate::ir::dead_branch::DeadBranch;
use crate::ir::edge::EdgeRecord;
use crate::ir::entity::{DbKind, Entity, EntityId, EntityNode};
use crate::ir::location::Location;
use crate::ir::plugin_meta::{PluginCommandCall, PluginMeta};
use crate::ir::self_switch::{SelfSwitchKey, SelfSwitchTable};
use crate::ir::symbols::{Site, SymbolTable};
use rustc_hash::{FxHashMap, FxHashSet};

/// Project engine, determined by the adapter.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    /// RPG Maker MV.
    Mv,
    /// RPG Maker MZ.
    Mz,
}

/// Vehicle kind (boat/ship/airship) for [`VehicleStartMap`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VehicleKind {
    /// Boat.
    Boat,
    /// Ship.
    Ship,
    /// Airship.
    Airship,
}

/// The fact "a vehicle starts on map N" (from System.json). The adapter puts
/// here only the vehicles that are set (startMapId != 0); the rule checks them
/// against maps_by_id.
#[derive(Clone, Debug, serde::Serialize)]
pub struct VehicleStartMap {
    /// Vehicle kind.
    pub vehicle: VehicleKind,
    /// Id of the vehicle's start map.
    pub map_id: u32,
    /// Location of the fact (System.json).
    pub location: Location,
}

/// Intermediate representation of the project: entity graph + typed edges
/// + symbol table + existence indices.
#[derive(Debug, serde::Serialize)]
pub struct Ir {
    /// Project engine.
    pub engine: Engine,
    /// Entity arena (indexed by [`EntityId`]).
    pub entities: Vec<EntityNode>,
    /// Flat list of edges.
    pub edges: Vec<EdgeRecord>,
    /// switch/var symbol table.
    pub symbols: SymbolTable,
    /// Self-switch table (a separate namespace from global switches).
    pub self_switches: SelfSwitchTable,
    /// `System.startMapId` — root of the reachability traversal.
    pub start_map_id: Option<u32>,
    /// Index: map id → entity.
    pub maps_by_id: FxHashMap<u32, EntityId>,
    /// Index: common event id → entity.
    pub common_events_by_id: FxHashMap<u32, EntityId>,
    /// Existence indices of DB records by kind and id.
    pub db: FxHashMap<DbKind, FxHashMap<u32, EntityId>>,
    /// Assets actually present on disk (after encryption normalization).
    pub assets_present: FxHashSet<AssetKey>,
    /// Assets that a plugin declares/provides (`@type file` parameters,
    /// plugin-managed roots such as busts): the plugin loads them itself
    /// from its own (possibly non-standard) folder. Therefore `broken-assets`
    /// does not raise an error on them, and `orphan-assets` does not count them
    /// as orphans.
    pub plugin_provided_assets: FxHashSet<AssetKey>,
    /// All asset reference sites.
    pub asset_refs: Vec<(AssetKey, Location)>,
    /// Plugin metadata (Tier A): load order, command registry, dependencies.
    pub plugin_meta: PluginMeta,
    /// Plugin command calls (356/357) from events — checked by the
    /// `unknown-plugin-command` rule against the `plugin_meta.commands` registry.
    pub plugin_command_calls: Vec<(PluginCommandCall, Location)>,
    /// Vehicle start maps (System.json) for existence checking.
    pub vehicle_start_maps: Vec<VehicleStartMap>,
    /// Maps referenced by a plugin (the profile declared them via a parameter —
    /// e.g. DK_Event_Factory template-event maps). `unreachable-maps` does not
    /// flag them: the player never visits them, the plugin uses them as a source.
    pub plugin_referenced_maps: FxHashSet<u32>,
    /// Constant-resolvable conditions (dead branches): computed by the adapter's
    /// light constant-propagation over 122 literals. Input to the
    /// `impossible-condition` rule.
    pub dead_branches: Vec<DeadBranch>,
    /// Ids of common events **reserved** by a script/plugin via a literal
    /// `$gameTemp.reserveCommonEvent(N)` (Tier B). `dead-common-event` does not
    /// flag them: such an event runs deferred, which the static analysis cannot
    /// see as a 117.
    pub reserved_common_events: FxHashSet<u32>,
}

impl Ir {
    /// Iterator over edges originating from the given entity.
    pub fn edges_from(&self, e: EntityId) -> impl Iterator<Item = &EdgeRecord> {
        self.edges.iter().filter(move |r| r.from == e)
    }

    /// Whether a DB record of the given kind with the given id exists.
    pub fn db_exists(&self, kind: DbKind, id: u32) -> bool {
        self.db.get(&kind).is_some_and(|m| m.contains_key(&id))
    }

    /// Access an entity by id (if within the arena bounds).
    pub fn entity(&self, id: EntityId) -> Option<&EntityNode> {
        self.entities.get(id.0 as usize)
    }

    /// Creates an empty IR builder for the adapter.
    pub fn builder(engine: Engine) -> IrBuilder {
        IrBuilder::new(engine)
    }
}

/// Builder for [`Ir`]: the adapter adds entities/edges/sites, then
/// [`IrBuilder::finish`] finalizes the indices.
pub struct IrBuilder {
    engine: Engine,
    entities: Vec<EntityNode>,
    edges: Vec<EdgeRecord>,
    symbols: SymbolTable,
    self_switches: SelfSwitchTable,
    start_map_id: Option<u32>,
    assets_present: FxHashSet<AssetKey>,
    plugin_provided_assets: FxHashSet<AssetKey>,
    asset_refs: Vec<(AssetKey, Location)>,
    plugin_meta: PluginMeta,
    plugin_command_calls: Vec<(PluginCommandCall, Location)>,
    vehicle_start_maps: Vec<VehicleStartMap>,
    plugin_referenced_maps: FxHashSet<u32>,
    dead_branches: Vec<DeadBranch>,
    reserved_common_events: FxHashSet<u32>,
}

impl IrBuilder {
    /// Creates an empty builder for the given engine.
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            entities: Vec::new(),
            edges: Vec::new(),
            symbols: SymbolTable::default(),
            self_switches: SelfSwitchTable::default(),
            start_map_id: None,
            assets_present: FxHashSet::default(),
            plugin_provided_assets: FxHashSet::default(),
            asset_refs: Vec::new(),
            plugin_meta: PluginMeta::default(),
            plugin_command_calls: Vec::new(),
            vehicle_start_maps: Vec::new(),
            plugin_referenced_maps: FxHashSet::default(),
            dead_branches: Vec::new(),
            reserved_common_events: FxHashSet::default(),
        }
    }

    /// Adds an entity and returns the [`EntityId`] assigned to it.
    pub fn push_entity(&mut self, kind: Entity, location: Location) -> EntityId {
        let id = EntityId(self.entities.len() as u32);
        self.entities.push(EntityNode { id, kind, location });
        id
    }

    /// Adds an edge originating from entity `from`.
    pub fn push_edge(&mut self, from: EntityId, edge: crate::ir::edge::Edge, location: Location) {
        self.edges.push(EdgeRecord {
            from,
            edge,
            location,
        });
    }

    /// Registers the start map (`System.startMapId`).
    pub fn set_start_map(&mut self, map_id: Option<u32>) {
        self.start_map_id = map_id;
    }

    /// Marks an asset as present on disk.
    pub fn add_asset_present(&mut self, key: AssetKey) {
        self.assets_present.insert(key);
    }

    /// Marks an asset as provided/managed by a plugin: `broken-assets`
    /// does not treat a reference to it as broken, and `orphan-assets` does not
    /// count it as an orphan.
    pub fn add_plugin_provided_asset(&mut self, key: AssetKey) {
        self.plugin_provided_assets.insert(key);
    }

    /// Sets the plugin metadata (Tier A).
    pub fn set_plugin_meta(&mut self, meta: PluginMeta) {
        self.plugin_meta = meta;
    }

    /// Mutable access to the plugin metadata for post-processing (curated
    /// profiles amend the command registry / order declarations after
    /// [`set_plugin_meta`](Self::set_plugin_meta) has run).
    pub fn plugin_meta_mut(&mut self) -> &mut PluginMeta {
        &mut self.plugin_meta
    }

    /// Registers a plugin command call (356/357) with its location.
    pub fn add_plugin_command_call(&mut self, call: PluginCommandCall, location: Location) {
        self.plugin_command_calls.push((call, location));
    }

    /// Registers a vehicle's start map (startMapId != 0).
    pub fn add_vehicle_start_map(&mut self, vehicle: VehicleKind, map_id: u32, location: Location) {
        self.vehicle_start_maps.push(VehicleStartMap {
            vehicle,
            map_id,
            location,
        });
    }

    /// Marks a map as used by a plugin (a source per the profile declaration):
    /// `unreachable-maps` does not flag it.
    pub fn add_plugin_referenced_map(&mut self, map_id: u32) {
        self.plugin_referenced_maps.insert(map_id);
    }

    /// Registers a constant-resolvable condition (a dead branch) for the
    /// `impossible-condition` rule.
    pub fn add_dead_branch(&mut self, branch: DeadBranch) {
        self.dead_branches.push(branch);
    }

    /// Marks a common event as reserved by a script/plugin
    /// (`$gameTemp.reserveCommonEvent(N)`): `dead-common-event` does not flag it.
    pub fn add_reserved_common_event(&mut self, id: u32) {
        if id != 0 {
            self.reserved_common_events.insert(id);
        }
    }

    /// Registers an asset reference site.
    pub fn add_asset_ref(&mut self, key: AssetKey, location: Location) {
        self.asset_refs.push((key, location));
    }

    /// Mutable access to the symbol table for populating sites.
    pub fn symbols_mut(&mut self) -> &mut SymbolTable {
        &mut self.symbols
    }

    /// Registers a self-switch read site keyed by `(map_id, event_id, ch)`.
    pub fn add_self_switch_read(&mut self, key: SelfSwitchKey, site: Site) {
        self.self_switches.add_read(key, site);
    }

    /// Registers a self-switch write site keyed by `(map_id, event_id, ch)`.
    pub fn add_self_switch_write(&mut self, key: SelfSwitchKey, site: Site) {
        self.self_switches.add_write(key, site);
    }

    /// Read-only access to already-added entities
    /// (convenient for the adapter when wiring up nested references).
    pub fn entities(&self) -> &[EntityNode] {
        &self.entities
    }

    /// Set of assets present on disk (for post-processing by profiles).
    pub fn assets_present(&self) -> &FxHashSet<AssetKey> {
        &self.assets_present
    }

    /// Asset reference sites (for post-processing by profiles).
    pub fn asset_refs(&self) -> &[(AssetKey, Location)] {
        &self.asset_refs
    }

    /// Finalizes the IR: builds the `maps_by_id`, `common_events_by_id`, `db` indices.
    pub fn finish(self) -> Ir {
        let mut maps_by_id = FxHashMap::default();
        let mut common_events_by_id = FxHashMap::default();
        let mut db: FxHashMap<DbKind, FxHashMap<u32, EntityId>> = FxHashMap::default();

        for node in &self.entities {
            match &node.kind {
                Entity::Map(m) => {
                    maps_by_id.insert(m.map_id, node.id);
                }
                Entity::CommonEvent(ce) => {
                    common_events_by_id.insert(ce.id, node.id);
                    // Register the common event's existence in `db` too, so that
                    // `db_exists(CommonEvent, id)` works for FK edges
                    // (effects 44, commands 117, etc.). Mirrors the Troop fix.
                    db.entry(DbKind::CommonEvent)
                        .or_default()
                        .insert(ce.id, node.id);
                }
                Entity::DatabaseRecord(rec) => {
                    db.entry(rec.kind)
                        .or_default()
                        .insert(rec.record_id, node.id);
                }
                Entity::Troop(t) => {
                    db.entry(DbKind::Troop).or_default().insert(t.id, node.id);
                }
                _ => {}
            }
        }

        Ir {
            engine: self.engine,
            entities: self.entities,
            edges: self.edges,
            symbols: self.symbols,
            self_switches: self.self_switches,
            start_map_id: self.start_map_id,
            maps_by_id,
            common_events_by_id,
            db,
            assets_present: self.assets_present,
            plugin_provided_assets: self.plugin_provided_assets,
            asset_refs: self.asset_refs,
            plugin_meta: self.plugin_meta,
            plugin_command_calls: self.plugin_command_calls,
            vehicle_start_maps: self.vehicle_start_maps,
            plugin_referenced_maps: self.plugin_referenced_maps,
            dead_branches: self.dead_branches,
            reserved_common_events: self.reserved_common_events,
        }
    }
}
