//! Root IR container ([`Ir`]) and its construction.
//!
//! The IR is built once and queried many times. Entities are an arena `Vec`,
//! edges are a flat `Vec` plus targeted indices (`maps_by_id`, `db`, …)
//! that are finalized after population. The adapter uses [`IrBuilder`].

use crate::ir::asset::AssetKey;
use crate::ir::common_event_summary::CommonEventSummary;
use crate::ir::dead_branch::DeadBranch;
use crate::ir::edge::{Edge, EdgeRecord};
use crate::ir::entity::{CeTrigger, DbKind, Entity, EntityId, EntityNode};
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

/// How a picture is operated on before it exists (for [`PictureMisuse`]).
///
/// A Move/Rotate/Tint/Erase Picture command targeting a picture id that has not
/// been Shown yet on the same straight-line command sequence. Engine-independent:
/// the adapter maps the numeric command codes (232/233/234/235) to these.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PictureOp {
    /// Move Picture (232).
    Move,
    /// Rotate Picture (233).
    Rotate,
    /// Tint Picture (234).
    Tint,
    /// Erase Picture (235).
    Erase,
}

/// The fact "a picture is operated on before it is shown" within one command list
/// (Tier: static data over a single straight-line sequence). Produced by the
/// adapter (it holds the picture ids and command ordering); consumed by the
/// `picture-lifecycle` rule. The operation runs on a picture that does not exist
/// yet, so it is a no-op / logic mistake.
#[derive(Clone, Debug, serde::Serialize)]
pub struct PictureMisuse {
    /// Picture id (RPG Maker picture slot).
    pub picture_id: u32,
    /// The offending operation.
    pub op: PictureOp,
    /// Location of the offending command.
    pub location: Location,
}

/// Which start position lands on an impassable tile (for [`BlockedTile`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockedTileKind {
    /// A Transfer Player (201) destination.
    Transfer,
    /// The player's start position (System.json startMapId/startX/startY).
    PlayerStart,
}

/// The fact "a fixed destination lands on a tile impassable from all four
/// directions" — the player would be unable to move off it (soft-lock).
///
/// Produced by the adapter's spatial pass (RPG Maker tileset passage flags live
/// there); consumed by the `blocked-tile` rule. Confidence is `likely`: passability
/// plugins (region passage, pixel movement) are not accounted for.
#[derive(Clone, Debug, serde::Serialize)]
pub struct BlockedTile {
    /// Which kind of destination this is.
    pub kind: BlockedTileKind,
    /// Target map id.
    pub map_id: u32,
    /// Target tile x (in tiles).
    pub x: u32,
    /// Target tile y (in tiles).
    pub y: u32,
    /// Location of the fact (the transfer command / System.json).
    pub location: Location,
}

/// A place that turns a global switch **ON** (Control Switches, 121) together with
/// the global switches that must already be ON for that place to run (its "gate":
/// the map-event page's switch conditions, or a triggered common event's switch).
///
/// Produced by the adapter; consumed by the `circular-gate` rule to detect
/// progression deadlocks — a switch whose only enablers are locked behind switches
/// that (transitively) require it, so it can never be turned on. An **empty** gate
/// means the setter is not switch-gated (the switch is freely settable).
#[derive(Clone, Debug, serde::Serialize)]
pub struct SwitchGate {
    /// The switch this place turns ON.
    pub switch_id: u32,
    /// Global switches required to be ON for the place to run.
    pub gate: Vec<u32>,
    /// Location of the setter (for the finding's related sites).
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
    /// Per-common-event behavioral summaries (interprocedural: reachability and
    /// exit-provision over the CommonEvent→CommonEvent call graph). Computed once
    /// in [`IrBuilder::finish`]; keyed by common-event id. Consumed by
    /// `dead-common-event` (reachability) and `stuck-autorun` (a `117` that hides
    /// no exit). See [`CommonEventSummary`].
    pub common_event_summaries: FxHashMap<u32, CommonEventSummary>,
    /// Switch-ON setters with their activation gate (121 ON writes from map-event
    /// pages / triggered common events). Input to the `circular-gate` rule.
    pub switch_gates: Vec<SwitchGate>,
    /// Switches written by an opaque source with an unknown value (an event
    /// script / plugin-command block, Tier B). `circular-gate` treats them as
    /// freely settable, since we cannot prove they are only turned on behind a
    /// gate.
    pub script_written_switches: FxHashSet<u32>,
    /// Fixed destinations (transfers / player start) that land on a tile
    /// impassable from all four directions. Input to the `blocked-tile` rule.
    pub blocked_tiles: Vec<BlockedTile>,
    /// Picture operations (Move/Rotate/Tint/Erase) that run before the picture is
    /// Shown on the same command sequence. Input to the `picture-lifecycle` rule.
    pub picture_misuses: Vec<PictureMisuse>,
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
    opaque_common_events: FxHashSet<u32>,
    switch_gates: Vec<SwitchGate>,
    script_written_switches: FxHashSet<u32>,
    blocked_tiles: Vec<BlockedTile>,
    picture_misuses: Vec<PictureMisuse>,
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
            opaque_common_events: FxHashSet::default(),
            switch_gates: Vec::new(),
            script_written_switches: FxHashSet::default(),
            blocked_tiles: Vec::new(),
            picture_misuses: Vec::new(),
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

    /// Registers a switch-ON setter with its activation gate (for the
    /// `circular-gate` rule). Ignores the placeholder switch id 0.
    pub fn add_switch_gate(&mut self, gate: SwitchGate) {
        if gate.switch_id != 0 {
            self.switch_gates.push(gate);
        }
    }

    /// Marks a switch as written by an opaque source (event script / plugin
    /// command) with an unknown value: `circular-gate` treats it as freely
    /// settable. Ignores the placeholder switch id 0.
    pub fn mark_switch_script_written(&mut self, id: u32) {
        if id != 0 {
            self.script_written_switches.insert(id);
        }
    }

    /// Marks a common event as reserved by a script/plugin
    /// (`$gameTemp.reserveCommonEvent(N)`): `dead-common-event` does not flag it.
    pub fn add_reserved_common_event(&mut self, id: u32) {
        if id != 0 {
            self.reserved_common_events.insert(id);
        }
    }

    /// Marks a common event as **opaque** — its command list contains a script or
    /// plugin command (355/356/357), so its effect on game state is not statically
    /// known. Only the adapter can supply this (it holds the numeric command
    /// codes); [`finish`](Self::finish) folds it into the event's summary.
    pub fn mark_common_event_opaque(&mut self, id: u32) {
        if id != 0 {
            self.opaque_common_events.insert(id);
        }
    }

    /// Registers an asset reference site.
    pub fn add_asset_ref(&mut self, key: AssetKey, location: Location) {
        self.asset_refs.push((key, location));
    }

    /// Registers a fixed destination that lands on a fully-blocked tile (for the
    /// `blocked-tile` rule).
    pub fn add_blocked_tile(&mut self, tile: BlockedTile) {
        self.blocked_tiles.push(tile);
    }

    /// Registers a picture operation that runs before the picture is shown (for
    /// the `picture-lifecycle` rule).
    pub fn add_picture_misuse(&mut self, misuse: PictureMisuse) {
        self.picture_misuses.push(misuse);
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

        let common_event_summaries = build_common_event_summaries(
            &self.entities,
            &self.edges,
            &self.reserved_common_events,
            &self.opaque_common_events,
        );

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
            common_event_summaries,
            switch_gates: self.switch_gates,
            script_written_switches: self.script_written_switches,
            blocked_tiles: self.blocked_tiles,
            picture_misuses: self.picture_misuses,
        }
    }
}

/// Builds the per-common-event summaries (locals + transitive verdicts) from the
/// finalized entity/edge arena. Split out of [`IrBuilder::finish`] for clarity.
///
/// Locals (`writes_switch`/`transfers`/`calls`/…) come from the event's outgoing
/// edges; `opaque` is supplied by the adapter (it holds the command codes). Two
/// fixed-point passes over the CommonEvent→CommonEvent call graph then compute
/// `provides_exit` (does a `117` hide a possible exit?) and `reachable` (can the
/// event ever run?). Both passes are monotone, so cycles converge safely.
fn build_common_event_summaries(
    entities: &[EntityNode],
    edges: &[EdgeRecord],
    reserved: &FxHashSet<u32>,
    opaque: &FxHashSet<u32>,
) -> FxHashMap<u32, CommonEventSummary> {
    // entity id → common-event id, plus per-event trigger and the id set.
    let mut ce_id_of: FxHashMap<EntityId, u32> = FxHashMap::default();
    let mut summaries: FxHashMap<u32, CommonEventSummary> = FxHashMap::default();
    let mut triggered: FxHashSet<u32> = FxHashSet::default();
    for node in entities {
        if let Entity::CommonEvent(ce) = &node.kind {
            ce_id_of.insert(node.id, ce.id);
            summaries
                .entry(ce.id)
                .or_insert_with(|| CommonEventSummary::new(ce.id, opaque.contains(&ce.id)));
            if ce.trigger != CeTrigger::None {
                triggered.insert(ce.id);
            }
        }
    }

    // Local facts from each event's outgoing edges + roots seeded by any
    // call/reference that originates OUTSIDE the common-event call graph
    // (a map/troop event 117, an effect-44 DB ref, a plugin reference).
    let mut roots: FxHashSet<u32> = FxHashSet::default();
    for rec in edges {
        let from_ce = ce_id_of.get(&rec.from).copied();
        match &rec.edge {
            Edge::CallsCommonEvent { common_event_id } => match from_ce {
                Some(src) => {
                    if let Some(s) = summaries.get_mut(&src)
                        && !s.calls.contains(common_event_id)
                    {
                        s.calls.push(*common_event_id);
                    }
                }
                None => {
                    roots.insert(*common_event_id);
                }
            },
            // An effect-44 / plugin reference from a non-common-event entity is a
            // live entry point into the callee.
            Edge::ReferencesDbId {
                kind: DbKind::CommonEvent,
                id,
            } if from_ce.is_none() => {
                roots.insert(*id);
            }
            _ => {}
        }
        let Some(src) = from_ce else { continue };
        let Some(s) = summaries.get_mut(&src) else {
            continue;
        };
        match &rec.edge {
            Edge::WritesSwitch { .. } => s.writes_switch = true,
            Edge::ReadsSwitch { .. } => s.reads_switch = true,
            Edge::WritesVariable { .. } => s.writes_variable = true,
            Edge::ReadsVariable { .. } => s.reads_variable = true,
            Edge::Transfer { .. } => s.transfers = true,
            _ => {}
        }
    }

    // Pass 1 — provides_exit fixed point. Seed with the local verdict; propagate
    // "a callee provides an exit" up the graph. A call to an unknown id (no
    // summary) is treated as exit-providing: we cannot prove it is a no-op.
    for id in summaries.keys().copied().collect::<Vec<_>>() {
        let local = summaries[&id].local_provides_exit();
        summaries.get_mut(&id).unwrap().provides_exit = local;
    }
    let mut changed = true;
    while changed {
        changed = false;
        for id in summaries.keys().copied().collect::<Vec<_>>() {
            if summaries[&id].provides_exit {
                continue;
            }
            let calls = summaries[&id].calls.clone();
            let exits = calls
                .iter()
                .any(|c| summaries.get(c).is_none_or(|s| s.provides_exit));
            if exits {
                summaries.get_mut(&id).unwrap().provides_exit = true;
                changed = true;
            }
        }
    }

    // Pass 2 — reachability. Roots: triggered events, reserved events, and events
    // entered from outside the call graph. Then walk `calls` edges transitively.
    for id in triggered.iter().chain(reserved.iter()) {
        roots.insert(*id);
    }
    let mut stack: Vec<u32> = roots
        .iter()
        .copied()
        .filter(|id| summaries.contains_key(id))
        .collect();
    while let Some(id) = stack.pop() {
        let calls = match summaries.get_mut(&id) {
            Some(s) if !s.reachable => {
                s.reachable = true;
                s.calls.clone()
            }
            _ => continue,
        };
        for c in calls {
            if summaries.contains_key(&c) {
                stack.push(c);
            }
        }
    }

    summaries
}
