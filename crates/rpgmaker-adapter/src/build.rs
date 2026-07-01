//! IR build orchestrator: root/engine detection, `data/` parsing, command
//! interpretation, asset scan, collection of asset references from data and commands.
//!
//! Parse errors of individual files are not fatal — they are collected into a list
//! of warnings, and the project is still built from whatever could be read.

use crate::command::EventCommand;
use crate::raw::{common_event::CommonEvent, database, map, plugins, system::System};
use crate::{assets, db_edges, interpreter};
use camino::{Utf8Path, Utf8PathBuf};
use dk_doctor_core::ir::{
    AssetKey, AssetKind, CeTrigger, CommandMeta, DbKind, Edge, Engine, Entity, EntityId, Ir,
    IrBuilder, Location, PageConditions, PageTrigger, PathSeg, Site,
};

/// Builds `Vec<CommandMeta>` for a page — a snapshot of the code/indent/index/location
/// of each command (needed by the `dead-code-after-exit` rule, so it doesn't re-read
/// the source).
fn command_meta(file: &Utf8Path, base: &[PathSeg], list: &[EventCommand]) -> Vec<CommandMeta> {
    list.iter()
        .enumerate()
        .map(|(i, cmd)| {
            let mut segs = base.to_vec();
            segs.push(PathSeg::Command(i as u32));
            CommandMeta {
                code: cmd.code,
                indent: cmd.indent,
                index: i as u32,
                location: Location::new(file.to_path_buf(), segs),
            }
        })
        .collect()
}

/// Warnings accumulated during loading (broken files, etc.).
#[derive(Debug, Default)]
pub struct LoadWarnings {
    /// List of messages about parse problems of individual files.
    pub messages: Vec<String>,
    /// How many core data files could be parsed as JSON (System/DB/common
    /// events/maps). `0` ⇒ data is unreadable (cipher/foreign format).
    pub parsed_files: usize,
}

/// Result of project layout detection: data root and asset root.
pub(crate) struct Layout {
    /// The root relative to which `data/`, `img/`, … reside (root or www/).
    base: Utf8PathBuf,
}

impl Layout {
    pub(crate) fn data_dir(&self) -> Utf8PathBuf {
        self.base.join("data")
    }

    /// Asset root (root or `www/`) — the base for `img/`, `audio/`, …
    pub(crate) fn asset_root(&self) -> &Utf8Path {
        &self.base
    }
}

/// Determines the project layout: tries the root, then `www/`.
pub(crate) fn detect_layout(root: &Utf8Path) -> Option<Layout> {
    if root.join("data").is_dir() {
        return Some(Layout {
            base: root.to_path_buf(),
        });
    }
    let www = root.join("www");
    if www.join("data").is_dir() {
        return Some(Layout { base: www });
    }
    None
}

/// Reads a file into a string, returning `None` (and an empty string) on error.
fn read_to_string(path: &Utf8Path) -> Option<String> {
    std::fs::read_to_string(path.as_std_path()).ok()
}

/// Main entry point for building IR from an already-determined data root.
pub fn build(root: &Utf8Path) -> Result<(Ir, LoadWarnings), crate::AdapterError> {
    let Some(layout) = detect_layout(root) else {
        return Err(crate::AdapterError::ProjectNotFound(root.to_string()));
    };
    let data = layout.data_dir();
    let mut warns = LoadWarnings::default();

    // System.json.
    let system: System = match read_to_string(&data.join("System.json")) {
        Some(text) => match serde_json::from_str(&text) {
            Ok(s) => {
                warns.parsed_files += 1;
                s
            }
            Err(e) => {
                warns.messages.push(format!("System.json: {e}"));
                System::default()
            }
        },
        None => {
            warns
                .messages
                .push("System.json: файл не найден".to_string());
            System::default()
        }
    };

    let engine = detect_engine(&layout, &data, &system);
    let mut b = Ir::builder(engine);

    // Symbol table from System.
    declare_symbols(&mut b, &system);
    b.symbols_mut().max_switch_id = system.max_switch_id();
    b.symbols_mut().max_variable_id = system.max_variable_id();
    b.set_start_map(if system.start_map_id != 0 {
        Some(system.start_map_id)
    } else {
        None
    });

    // Vehicle start maps (boat/ship/airship): later checked by the
    // `vehicle-start-map` rule. We record only the ones that are set (startMapId != 0).
    {
        use dk_doctor_core::ir::VehicleKind;
        let sys_loc = Location::file_only("data/System.json");
        for (kind, veh) in [
            (VehicleKind::Boat, &system.boat),
            (VehicleKind::Ship, &system.ship),
            (VehicleKind::Airship, &system.airship),
        ] {
            if veh.start_map_id != 0 {
                b.add_vehicle_start_map(kind, veh.start_map_id, sys_loc.clone());
            }
        }
    }

    // Database.
    load_database(&mut b, &data, &mut warns);

    // Common events.
    load_common_events(&mut b, &data, &mut warns);

    // Maps.
    load_maps(&mut b, &data, &mut warns);

    // Enemy groups (Troops).
    load_troops(&mut b, &data, &mut warns);

    // Asset references from data fields (System/Actors/Enemies/Tilesets/Map already partly above).
    collect_system_asset_refs(&mut b, &system);

    // Scan of assets present on disk.
    for key in assets::scan(&layout.base) {
        b.add_asset_present(key);
    }

    // No core file parsed ⇒ data is encrypted non-standardly
    // (OMORI `.KEL`) or this isn't RPG Maker. An explicit signal instead of "0 problems".
    if warns.parsed_files == 0 {
        return Err(crate::AdapterError::NoAnalyzableData(root.to_string()));
    }

    // Plugins (Tier A/B): parsing annotations + JS of enabled plugins →
    // declared_by_plugin, provided assets, command registry, patches, load order.
    let (plugins, plugins_js) = collect_plugins(&mut b, &layout, &mut warns);

    // Profiles + `.dk-doctor`: post-processing of facts (asset_roots/localization,
    // ignore globs, map_param, and the curated per-plugin param/command/dependency
    // tables). Runs ALWAYS (ignore globs work even without plugins).
    crate::profiles::apply(
        &mut b,
        &layout.base,
        &plugins_js,
        &plugins,
        &mut warns.messages,
    );

    Ok((b.finish(), warns))
}

/// Parses `plugins.js` + headers/JS of enabled plugins and folds Tier-A/B
/// facts into the IR. Absence of `plugins.js` is not an error (a project without plugins).
/// Returns ALL plugins (name + `parameters` + whether enabled, in load order) —
/// for post-processing by profiles (`provided_subdir`/`map_param` read parameter
/// values; `map_param` applies to disabled plugins too) — together with the
/// `plugins.js` path relative to the project base (for profile-emitted edge
/// locations).
fn collect_plugins(
    b: &mut IrBuilder,
    layout: &Layout,
    warns: &mut LoadWarnings,
) -> (Vec<crate::plugins::PluginParams>, Utf8PathBuf) {
    let candidates = [
        layout.base.join("js").join("plugins.js"),
        layout.base.join("plugins.js"),
    ];
    let mut entries = Vec::new();
    let mut plugins_dir = layout.base.join("js").join("plugins");
    // Path of plugins.js relative to the base — for locations of plugin edges (PathSeg::Plugin).
    let mut plugins_js = Utf8PathBuf::from("js/plugins.js");
    for path in &candidates {
        if let Some(text) = read_to_string(path) {
            entries = plugins::parse(&text);
            // The plugins directory is next to plugins.js.
            if let Some(parent) = path.parent() {
                plugins_dir = parent.join("plugins");
            }
            if let Ok(rel) = path.strip_prefix(&layout.base) {
                // Locations use slashes (like `data/MapXXX.json`), not Windows `\`.
                plugins_js = Utf8PathBuf::from(rel.as_str().replace('\\', "/"));
            }
            break;
        }
    }
    if entries.is_empty() {
        return (Vec::new(), plugins_js);
    }
    crate::plugins::collect::collect(b, &entries, &plugins_dir, &plugins_js, &mut warns.messages);
    let plugins = entries
        .iter()
        .map(|p| (p.name.clone(), p.parameters.clone(), p.status))
        .collect();
    (plugins, plugins_js)
}

/// Engine heuristic: first by core scripts (reliable), then `effects/` and 357.
///
/// The `advanced` field is NOT used for detection — it exists in MV 1.5+ too, which
/// caused new MV projects to be wrongly detected as MZ.
fn detect_engine(layout: &Layout, data: &Utf8Path, _system: &System) -> Engine {
    let js = layout.base.join("js");
    if js.join("rmmz_core.js").is_file() {
        return Engine::Mz;
    }
    if js.join("rpg_core.js").is_file() || js.join("rpg_objects.js").is_file() {
        return Engine::Mv;
    }
    // The effects/ folder is MZ-only (Effekseer).
    if layout.base.join("effects").is_dir() {
        return Engine::Mz;
    }
    // Sign of 357 in common events (a cheap single-file check).
    if let Some(text) = read_to_string(&data.join("CommonEvents.json"))
        && (text.contains("\"code\":357") || text.contains("\"code\": 357"))
    {
        return Engine::Mz;
    }
    Engine::Mv
}

fn declare_symbols(b: &mut IrBuilder, system: &System) {
    for (id, name) in system.switches.iter().enumerate() {
        if id == 0 {
            continue;
        }
        if let Some(n) = name {
            b.symbols_mut().declare_switch(id as u32, Some(n.clone()));
        }
    }
    for (id, name) in system.variables.iter().enumerate() {
        if id == 0 {
            continue;
        }
        if let Some(n) = name {
            b.symbols_mut().declare_variable(id as u32, Some(n.clone()));
        }
    }
}

fn load_database(b: &mut IrBuilder, data: &Utf8Path, warns: &mut LoadWarnings) {
    /// Loads a table and for each record: pushes the DB entity and calls
    /// `$emit` to emit its FK edges (`db_edges`).
    macro_rules! load_with_edges {
        ($kind:expr, $ty:ty, $emit:expr) => {{
            let file = format!("{}.json", $kind.file_stem());
            if let Some(text) = read_to_string(&data.join(&file)) {
                match database::parse_table::<$ty>(&text) {
                    Ok(table) => {
                        warns.parsed_files += 1;
                        for rec in table.into_iter().flatten() {
                            if rec.id == 0 {
                                continue;
                            }
                            let entity = push_db_entity(b, $kind, rec.id, rec.name.clone());
                            let loc = db_loc($kind, rec.id);
                            #[allow(clippy::redundant_closure_call)]
                            ($emit)(b, entity, &loc, &rec);
                        }
                    }
                    Err(e) => warns.messages.push(format!("{file}: {e}")),
                }
            }
        }};
    }

    load_with_edges!(DbKind::Actor, database::Actor, db_edges::actor);
    load_with_edges!(DbKind::Class, database::Class, db_edges::class);
    load_with_edges!(DbKind::Skill, database::Skill, db_edges::skill);
    load_with_edges!(DbKind::Item, database::Item, db_edges::item);
    load_with_edges!(DbKind::Weapon, database::Weapon, db_edges::weapon);
    load_with_edges!(DbKind::Armor, database::Armor, db_edges::armor);
    load_with_edges!(DbKind::State, database::State, db_edges::state);
    // Tilesets: records are needed for existence (db_exists); their image slots
    // (tilesetNames[9]) are emitted as asset refs so broken-assets flags a map's
    // tileset whose A1..E image is missing on disk.
    load_with_edges!(
        DbKind::Tileset,
        database::Tileset,
        |b: &mut IrBuilder, entity, loc: &Location, rec: &database::Tileset| {
            for name in &rec.tileset_names {
                add_data_asset(b, entity, AssetKind::Tileset, name, loc);
            }
        }
    );

    // Animations / Enemies / Troops are loaded separately (they need special fields/assets).
    load_animations(b, data, warns);
    load_enemies(b, data, warns);
}

fn db_file_path(kind: DbKind) -> Utf8PathBuf {
    Utf8PathBuf::from(format!("data/{}.json", kind.file_stem()))
}

fn db_loc(kind: DbKind, id: u32) -> Location {
    Location::new(
        db_file_path(kind),
        vec![PathSeg::DbRecord {
            file: kind.file_stem(),
            id,
        }],
    )
}

fn load_animations(b: &mut IrBuilder, data: &Utf8Path, warns: &mut LoadWarnings) {
    let file = "Animations.json";
    let Some(text) = read_to_string(&data.join(file)) else {
        return;
    };
    match database::parse_table::<database::Animation>(&text) {
        Ok(table) => {
            warns.parsed_files += 1;
            for anim in table.into_iter().flatten() {
                // Skip id 0 like every other DB loader: a non-null garbage object
                // at index 0 (missing `id`) would otherwise register a phantom
                // Animation #0 in the db index.
                if anim.id == 0 {
                    continue;
                }
                let entity = push_db_entity(b, DbKind::Animation, anim.id, anim.name.clone());
                let loc = db_loc(DbKind::Animation, anim.id);
                // Effekseer (MZ) vs MV style — by the presence of frames.
                if anim.frames.is_some() {
                    add_data_asset(b, entity, AssetKind::Animation, &anim.animation1_name, &loc);
                    add_data_asset(b, entity, AssetKind::Animation, &anim.animation2_name, &loc);
                } else if !anim.effect_name.is_empty() {
                    add_data_asset(b, entity, AssetKind::Effect, &anim.effect_name, &loc);
                }
            }
        }
        Err(e) => warns.messages.push(format!("{file}: {e}")),
    }
}

fn load_enemies(b: &mut IrBuilder, data: &Utf8Path, warns: &mut LoadWarnings) {
    let file = "Enemies.json";
    let Some(text) = read_to_string(&data.join(file)) else {
        return;
    };
    match database::parse_table::<database::Enemy>(&text) {
        Ok(table) => {
            warns.parsed_files += 1;
            for enemy in table.into_iter().flatten() {
                if enemy.id == 0 {
                    continue;
                }
                let entity = push_db_entity(b, DbKind::Enemy, enemy.id, enemy.name.clone());
                let loc = db_loc(DbKind::Enemy, enemy.id);
                // battlerName: front- or side-view — in iter1 we accept both folders.
                add_data_asset(b, entity, AssetKind::Enemy, &enemy.battler_name, &loc);
                add_data_asset(b, entity, AssetKind::SvEnemy, &enemy.battler_name, &loc);
                // FK edges: actions/dropItems/traits (§3.1/§3.2).
                db_edges::enemy(b, entity, &loc, &enemy);
            }
        }
        Err(e) => warns.messages.push(format!("{file}: {e}")),
    }
}

/// Version of [`push_db`] that returns an [`EntityId`] (for attaching asset references).
fn push_db_entity(b: &mut IrBuilder, kind: DbKind, id: u32, name: String) -> EntityId {
    b.push_entity(
        Entity::DatabaseRecord(dk_doctor_core::ir::DatabaseRecord {
            kind,
            record_id: id,
            name,
        }),
        db_loc(kind, id),
    )
}

/// Adds an asset reference from a data field (empty names are skipped).
fn add_data_asset(b: &mut IrBuilder, from: EntityId, kind: AssetKind, name: &str, loc: &Location) {
    if name.is_empty() {
        return;
    }
    let name = crate::assets::strip_bracket_prefix(name);
    if name.is_empty() {
        return;
    }
    let key = AssetKey::new(kind, name);
    b.add_asset_ref(key.clone(), loc.clone());
    b.push_edge(from, Edge::ReferencesAsset { asset: key }, loc.clone());
}

fn load_common_events(b: &mut IrBuilder, data: &Utf8Path, warns: &mut LoadWarnings) {
    let file = "CommonEvents.json";
    let Some(text) = read_to_string(&data.join(file)) else {
        return;
    };
    // parse_table skips individual malformed records instead of dropping the
    // whole file, so one bad common event cannot blind the rest.
    let table: Vec<Option<CommonEvent>> = match database::parse_table(&text) {
        Ok(t) => t,
        Err(e) => {
            warns.messages.push(format!("{file}: {e}"));
            return;
        }
    };
    warns.parsed_files += 1;
    for ce in table.into_iter().flatten() {
        if ce.id == 0 {
            continue;
        }
        let trigger = match ce.trigger {
            1 => CeTrigger::Autorun,
            2 => CeTrigger::Parallel,
            _ => CeTrigger::None,
        };
        let loc = Location::new(
            Utf8PathBuf::from("data/CommonEvents.json"),
            vec![PathSeg::CommonEvent(ce.id)],
        );
        let entity = b.push_entity(
            Entity::CommonEvent(dk_doctor_core::ir::CommonEvent {
                id: ce.id,
                name: ce.name.clone(),
                trigger,
                command_count: ce.list.len() as u32,
            }),
            loc.clone(),
        );
        // Opaque = the list contains a script / plugin command (355/356/357),
        // whose effect on game state static analysis cannot follow. Feeds the
        // common-event summary (`provides_exit`) computed in `finish`.
        if ce.list.iter().any(|c| {
            matches!(
                c.code,
                crate::codes::SCRIPT
                    | crate::codes::PLUGIN_COMMAND_MV
                    | crate::codes::PLUGIN_COMMAND_MZ
            )
        }) {
            b.mark_common_event_opaque(ce.id);
        }
        // switchId — READ when trigger!=0.
        if ce.trigger != 0 && ce.switch_id != 0 {
            b.symbols_mut().add_switch_read(
                ce.switch_id,
                Site {
                    location: loc.clone(),
                    entity,
                },
            );
            b.push_edge(
                entity,
                Edge::ReadsSwitch {
                    switch_id: ce.switch_id,
                },
                loc.clone(),
            );
        }
        // Gate for `circular-gate`: a triggered (Autorun/Parallel) common event
        // runs only while its switch is ON; a call-triggered event (trigger 0)
        // runs when invoked, so it is not switch-gated (empty gate).
        let gate_switches = if ce.trigger != 0 && ce.switch_id != 0 {
            vec![ce.switch_id]
        } else {
            Vec::new()
        };
        let ctx = interpreter::WalkCtx {
            entity,
            file: Utf8PathBuf::from("data/CommonEvents.json"),
            base_path: vec![PathSeg::CommonEvent(ce.id)],
            // Common events have no self-switches.
            self_switch_scope: None,
            gate_switches,
        };
        interpreter::walk(b, &ctx, &ce.list);
    }
}

fn load_maps(b: &mut IrBuilder, data: &Utf8Path, warns: &mut LoadWarnings) {
    // Map names are taken from MapInfos.
    let infos = read_to_string(&data.join("MapInfos.json"))
        .and_then(|t| serde_json::from_str::<Vec<Option<map::MapInfo>>>(&t).ok())
        .unwrap_or_default();
    let mut names: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    for info in infos.into_iter().flatten() {
        names.insert(info.id, info.name);
    }

    // We iterate over MapNNN.json by numbers from MapInfos and simply by directory files.
    let ids = collect_map_ids(data, &names);
    for map_id in ids {
        let file = format!("Map{map_id:03}.json");
        let Some(text) = read_to_string(&data.join(&file)) else {
            continue;
        };
        let m: map::Map = match serde_json::from_str(&text) {
            Ok(m) => m,
            Err(e) => {
                warns.messages.push(format!("{file}: {e}"));
                continue;
            }
        };
        warns.parsed_files += 1;
        load_one_map(b, map_id, &names, &m, &file);
    }
}

fn collect_map_ids(data: &Utf8Path, names: &std::collections::HashMap<u32, String>) -> Vec<u32> {
    let mut ids: std::collections::BTreeSet<u32> = names.keys().copied().collect();
    if let Ok(entries) = std::fs::read_dir(data.as_std_path()) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && let Some(rest) = name.strip_prefix("Map")
                && let Some(num) = rest.strip_suffix(".json")
                && let Ok(id) = num.parse::<u32>()
            {
                ids.insert(id);
            }
        }
    }
    ids.into_iter().filter(|&id| id != 0).collect()
}

/// Whether the map's events contain at least one command 301 (Battle Processing).
fn map_has_battle_processing(m: &map::Map) -> bool {
    m.events
        .iter()
        .flatten()
        .flat_map(|ev| ev.pages.iter())
        .flat_map(|page| page.list.iter())
        .any(|cmd| cmd.code == crate::codes::BATTLE_PROCESSING)
}

fn load_one_map(
    b: &mut IrBuilder,
    map_id: u32,
    names: &std::collections::HashMap<u32, String>,
    m: &map::Map,
    file: &str,
) {
    let file_path = Utf8PathBuf::from(format!("data/{file}"));
    let map_loc = Location::new(file_path.clone(), vec![PathSeg::Map(map_id)]);
    let name = names.get(&map_id).cloned().unwrap_or_default();

    // Whether the map can start a battle: a non-empty encounterList OR command 301 in any
    // command list of the map's event pages. Needed for usage-gating of battlebacks.
    let can_battle = !m.encounter_list.is_empty() || map_has_battle_processing(m);

    let map_entity = b.push_entity(
        Entity::Map(dk_doctor_core::ir::Map {
            map_id,
            name,
            event_ids: Vec::new(),
            can_battle,
        }),
        map_loc.clone(),
    );

    // Map FK edges: tilesetId → Tileset; encounterList[].troopId → Troop.
    if m.tileset_id != 0 {
        b.push_edge(
            map_entity,
            Edge::ReferencesDbId {
                kind: DbKind::Tileset,
                id: m.tileset_id,
            },
            map_loc.clone(),
        );
    }
    for enc in &m.encounter_list {
        if enc.troop_id != 0 {
            b.push_edge(
                map_entity,
                Edge::ReferencesDbId {
                    kind: DbKind::Troop,
                    id: enc.troop_id,
                },
                map_loc.clone(),
            );
        }
    }

    // Map asset fields.
    add_data_asset(
        b,
        map_entity,
        AssetKind::Battleback1,
        &m.battleback1_name,
        &map_loc,
    );
    add_data_asset(
        b,
        map_entity,
        AssetKind::Battleback2,
        &m.battleback2_name,
        &map_loc,
    );
    add_data_asset(
        b,
        map_entity,
        AssetKind::Parallax,
        &m.parallax_name,
        &map_loc,
    );
    add_data_asset(b, map_entity, AssetKind::Bgm, &m.bgm.name, &map_loc);
    add_data_asset(b, map_entity, AssetKind::Bgs, &m.bgs.name, &map_loc);

    for event in m.events.iter().flatten() {
        if event.id == 0 {
            continue;
        }
        let ev_loc = Location::new(
            file_path.clone(),
            vec![PathSeg::Map(map_id), PathSeg::Event(event.id)],
        );
        let ev_entity = b.push_entity(
            Entity::Event(dk_doctor_core::ir::Event {
                map_id,
                event_id: event.id,
                page_ids: Vec::new(),
            }),
            ev_loc,
        );
        for (pi, page) in event.pages.iter().enumerate() {
            let page_no = (pi + 1) as u32;
            let base = vec![
                PathSeg::Map(map_id),
                PathSeg::Event(event.id),
                PathSeg::Page(page_no),
            ];
            let page_loc = Location::new(file_path.clone(), base.clone());
            let conditions = page_conditions(&page.conditions);
            let page_entity = b.push_entity(
                Entity::Page(dk_doctor_core::ir::Page {
                    conditions: conditions.clone(),
                    trigger: PageTrigger::from_raw(page.trigger),
                    command_count: page.list.len() as u32,
                    commands: command_meta(&file_path, &base, &page.list),
                }),
                page_loc.clone(),
            );

            // Page conditions = READ sites of switch/var.
            emit_page_condition_reads(b, page_entity, &page_loc, &conditions);

            // selfSwitchValid → selfSwitchCh: READ self-switch of the current event.
            if let Some(ch) = conditions.self_switch
                && event.id != 0
            {
                b.add_self_switch_read(
                    dk_doctor_core::ir::SelfSwitchKey::new(map_id, event.id, ch),
                    Site {
                        location: page_loc.clone(),
                        entity: page_entity,
                    },
                );
            }

            // Event graphic (img/characters/) — asset reference when tileId==0.
            if page.image.tile_id == 0 && !page.image.character_name.is_empty() {
                add_data_asset(
                    b,
                    page_entity,
                    AssetKind::Character,
                    &page.image.character_name,
                    &page_loc,
                );
            }

            // Gate for `circular-gate`: the page's global-switch activation
            // conditions (switch1/switch2). Variable/self-switch/item/actor
            // conditions are intentionally omitted — ignoring them only makes a
            // setter look MORE reachable (fewer, not false, deadlock findings).
            let gate_switches = conditions
                .switch1
                .into_iter()
                .chain(conditions.switch2)
                .collect();
            let ctx = interpreter::WalkCtx {
                entity: page_entity,
                file: file_path.clone(),
                base_path: base,
                self_switch_scope: Some(interpreter::SelfSwitchScope {
                    map_id,
                    event_id: event.id,
                }),
                gate_switches,
            };
            interpreter::walk(b, &ctx, &page.list);
            let _ = ev_entity;
        }
    }
}

fn page_conditions(c: &map::PageConditions) -> PageConditions {
    let self_switch = if c.self_switch_valid {
        c.self_switch_ch.chars().next()
    } else {
        None
    };
    PageConditions {
        switch1: (c.switch1_valid && c.switch1_id != 0).then_some(c.switch1_id),
        switch2: (c.switch2_valid && c.switch2_id != 0).then_some(c.switch2_id),
        variable: (c.variable_valid && c.variable_id != 0).then_some(c.variable_id),
        variable_value: c.variable_valid.then_some(c.variable_value),
        self_switch,
        item: (c.item_valid && c.item_id != 0).then_some(c.item_id),
        actor: (c.actor_valid && c.actor_id != 0).then_some(c.actor_id),
    }
}

fn emit_page_condition_reads(
    b: &mut IrBuilder,
    entity: EntityId,
    loc: &Location,
    cond: &PageConditions,
) {
    let site = |entity| Site {
        location: loc.clone(),
        entity,
    };
    if let Some(id) = cond.switch1 {
        b.symbols_mut().add_switch_read(id, site(entity));
        b.push_edge(entity, Edge::ReadsSwitch { switch_id: id }, loc.clone());
    }
    if let Some(id) = cond.switch2 {
        b.symbols_mut().add_switch_read(id, site(entity));
        b.push_edge(entity, Edge::ReadsSwitch { switch_id: id }, loc.clone());
    }
    if let Some(id) = cond.variable {
        b.symbols_mut().add_variable_read(id, site(entity));
        b.push_edge(entity, Edge::ReadsVariable { variable_id: id }, loc.clone());
    }
    // item/actor page conditions — DB references.
    if let Some(id) = cond.item {
        b.push_edge(
            entity,
            Edge::ReferencesDbId {
                kind: DbKind::Item,
                id,
            },
            loc.clone(),
        );
    }
    if let Some(id) = cond.actor {
        b.push_edge(
            entity,
            Edge::ReferencesDbId {
                kind: DbKind::Actor,
                id,
            },
            loc.clone(),
        );
    }
}

fn load_troops(b: &mut IrBuilder, data: &Utf8Path, warns: &mut LoadWarnings) {
    let file = "Troops.json";
    let Some(text) = read_to_string(&data.join(file)) else {
        return;
    };
    // Per-record resilient parse: a single malformed troop is skipped, not fatal.
    let table: Vec<Option<database::Troop>> = match database::parse_table(&text) {
        Ok(t) => t,
        Err(e) => {
            warns.messages.push(format!("{file}: {e}"));
            return;
        }
    };
    warns.parsed_files += 1;
    let file_path = Utf8PathBuf::from("data/Troops.json");
    for troop in table.into_iter().flatten() {
        if troop.id == 0 {
            continue;
        }
        let troop_loc = Location::new(file_path.clone(), vec![PathSeg::Troop(troop.id)]);
        let troop_entity = b.push_entity(
            Entity::Troop(dk_doctor_core::ir::Troop { id: troop.id }),
            troop_loc.clone(),
        );
        // members[].enemyId → Enemies.
        for member in &troop.members {
            if member.enemy_id != 0 {
                b.push_edge(
                    troop_entity,
                    Edge::ReferencesDbId {
                        kind: DbKind::Enemy,
                        id: member.enemy_id,
                    },
                    troop_loc.clone(),
                );
            }
        }
        for (pi, page) in troop.pages.iter().enumerate() {
            let base = vec![PathSeg::Troop(troop.id), PathSeg::Page((pi + 1) as u32)];
            let page_loc = Location::new(file_path.clone(), base.clone());
            // Conditions: switchId READ, actorId DB reference.
            if page.conditions.switch_valid && page.conditions.switch_id != 0 {
                b.symbols_mut().add_switch_read(
                    page.conditions.switch_id,
                    Site {
                        location: page_loc.clone(),
                        entity: troop_entity,
                    },
                );
                b.push_edge(
                    troop_entity,
                    Edge::ReadsSwitch {
                        switch_id: page.conditions.switch_id,
                    },
                    page_loc.clone(),
                );
            }
            if page.conditions.actor_valid && page.conditions.actor_id != 0 {
                b.push_edge(
                    troop_entity,
                    Edge::ReferencesDbId {
                        kind: DbKind::Actor,
                        id: page.conditions.actor_id,
                    },
                    page_loc.clone(),
                );
            }
            let ctx = interpreter::WalkCtx {
                entity: troop_entity,
                file: file_path.clone(),
                base_path: base,
                // Troop pages have no self-switches.
                self_switch_scope: None,
                // Battle events are not map progression gates; a switch set here is
                // treated as freely settable (empty gate).
                gate_switches: Vec::new(),
            };
            interpreter::walk(b, &ctx, &page.list);
        }
    }
}

/// Asset references from `System.json` fields (titles, audio, vehicles).
///
/// `battleback1Name`/`battleback2Name` are NOT included: these are the editor's
/// **Battle Test** backgrounds, which aren't loaded in the normal game (the battle
/// background is taken from the map/troop). The absence of such a file is not a bug
/// (the player doesn't run Battle Test), so flagging it = a false broken-asset
/// (noise on projects that didn't ship the test background).
fn collect_system_asset_refs(b: &mut IrBuilder, system: &System) {
    let loc = Location::file_only("data/System.json");
    let mut refs: Vec<(AssetKind, &str)> = vec![
        (AssetKind::Title1, system.title1_name.as_str()),
        (AssetKind::Title2, system.title2_name.as_str()),
        (AssetKind::Bgm, system.title_bgm.name.as_str()),
        (AssetKind::Bgm, system.battle_bgm.name.as_str()),
        (AssetKind::Me, system.victory_me.name.as_str()),
        (AssetKind::Me, system.defeat_me.name.as_str()),
        (AssetKind::Me, system.gameover_me.name.as_str()),
        (AssetKind::Character, system.boat.character_name.as_str()),
        (AssetKind::Character, system.ship.character_name.as_str()),
        (AssetKind::Character, system.airship.character_name.as_str()),
        (AssetKind::Bgm, system.boat.bgm.name.as_str()),
        (AssetKind::Bgm, system.ship.bgm.name.as_str()),
        (AssetKind::Bgm, system.airship.bgm.name.as_str()),
    ];
    for sound in &system.sounds {
        refs.push((AssetKind::Se, sound.name.as_str()));
    }
    for (kind, name) in refs {
        if !name.is_empty() {
            b.add_asset_ref(AssetKey::new(kind, name), loc.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dk_doctor_core::ir::{Engine, Entity, Ir, PageTrigger};

    /// Runs [`load_one_map`] over a raw map from JSON and returns the finished IR.
    fn build_map(map_json: serde_json::Value) -> Ir {
        let m: map::Map = serde_json::from_value(map_json).unwrap();
        let mut b = Ir::builder(Engine::Mz);
        let names = std::collections::HashMap::new();
        load_one_map(&mut b, 1, &names, &m, "Map001.json");
        b.finish()
    }

    #[test]
    fn system_battleback_is_not_an_asset_ref() {
        // battleback1/2Name (Battle Test backgrounds) must not produce an asset-ref:
        // they aren't loaded in the normal game, otherwise — a false broken-asset.
        let system = System {
            title1_name: "Castle".to_string(),
            battleback1_name: "GrassMaze".to_string(),
            battleback2_name: "GrassMaze".to_string(),
            ..System::default()
        };
        let mut b = Ir::builder(Engine::Mz);
        collect_system_asset_refs(&mut b, &system);
        let ir = b.finish();
        assert!(
            ir.asset_refs
                .iter()
                .any(|(k, _)| k.kind == AssetKind::Title1 && k.name == "Castle"),
            "title1 (используется в игре) — ref есть"
        );
        assert!(
            !ir.asset_refs
                .iter()
                .any(|(k, _)| matches!(k.kind, AssetKind::Battleback1 | AssetKind::Battleback2)),
            "battleback теста боя — ref НЕ эмитится"
        );
    }

    #[test]
    fn page_trigger_round_trips() {
        // trigger 3 (Autorun) and 4 (Parallel) in two pages of the same event.
        let ir = build_map(serde_json::json!({
            "events": [null, {
                "id": 1,
                "name": "EV",
                "pages": [
                    {"trigger": 3, "list": []},
                    {"trigger": 4, "list": []}
                ]
            }]
        }));
        let triggers: Vec<PageTrigger> = ir
            .entities
            .iter()
            .filter_map(|n| match &n.kind {
                Entity::Page(p) => Some(p.trigger),
                _ => None,
            })
            .collect();
        assert_eq!(triggers, vec![PageTrigger::Autorun, PageTrigger::Parallel]);
    }

    fn map_can_battle(ir: &Ir) -> bool {
        ir.entities
            .iter()
            .find_map(|n| match &n.kind {
                Entity::Map(m) => Some(m.can_battle),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn map_with_encounters_can_battle() {
        let ir = build_map(serde_json::json!({
            "encounterList": [{"troopId": 2}],
            "events": [],
        }));
        assert!(map_can_battle(&ir));
    }

    #[test]
    fn map_with_battle_processing_command_can_battle() {
        // No encounterList, but command 301 (Battle Processing) in the event.
        let ir = build_map(serde_json::json!({
            "events": [null, {
                "id": 1,
                "name": "EV",
                "pages": [
                    {"trigger": 0, "list": [
                        {"code": 301, "indent": 0, "parameters": [0, 3, false, false]}
                    ]}
                ]
            }]
        }));
        assert!(map_can_battle(&ir));
    }

    #[test]
    fn map_without_battle_capability_cannot_battle() {
        let ir = build_map(serde_json::json!({
            "events": [null, {
                "id": 1,
                "name": "EV",
                "pages": [
                    {"trigger": 0, "list": [
                        {"code": 101, "indent": 0, "parameters": ["", 0, 0, 2, ""]}
                    ]}
                ]
            }]
        }));
        assert!(!map_can_battle(&ir));
    }

    /// A unique temporary directory for the build test.
    fn temp_root(tag: &str) -> Utf8PathBuf {
        let p = std::env::temp_dir().join(format!(
            "dkbuild_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        Utf8PathBuf::from_path_buf(p).unwrap()
    }

    #[test]
    fn unreadable_data_yields_no_analyzable_data_error() {
        // data/ exists, but the only "JSON" is garbage (imitating .KEL/a cipher).
        let root = temp_root("unreadable");
        let data = root.join("data");
        std::fs::create_dir_all(data.as_std_path()).unwrap();
        std::fs::write(
            data.join("System.json").as_std_path(),
            b"\x00\x01KELgarbage",
        )
        .unwrap();
        std::fs::write(data.join("Map001.json").as_std_path(), b"not json at all").unwrap();

        let err = build(&root).unwrap_err();
        assert!(matches!(err, crate::AdapterError::NoAnalyzableData(_)));

        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn build_wires_plugin_tier_a_into_ir() {
        // Minimal valid project + plugins.js + one enabled plugin with
        // @type switch / @type file annotations.
        let root = temp_root("plugins");
        let data = root.join("data");
        std::fs::create_dir_all(data.as_std_path()).unwrap();
        // One parseable core file is enough for the project to be considered valid.
        std::fs::write(
            data.join("System.json").as_std_path(),
            br#"{"switches":["","S1"],"variables":["","V1"]}"#,
        )
        .unwrap();

        let js = root.join("js");
        let pdir = js.join("plugins");
        std::fs::create_dir_all(pdir.as_std_path()).unwrap();
        std::fs::write(
            js.join("plugins.js").as_std_path(),
            r#"var $plugins =
[
{"name":"OwnsSwitch","status":true,"description":"","parameters":{"Sw":"1","Pic":"banner"}},
{"name":"Disabled","status":false,"description":"","parameters":{}}
];"#,
        )
        .unwrap();
        std::fs::write(
            pdir.join("OwnsSwitch.js").as_std_path(),
            r#"/*:
 * @param Sw
 * @type switch
 * @param Pic
 * @type file
 * @dir img/pictures
 * @command bang
 */"#,
        )
        .unwrap();

        let (ir, _warns) = build(&root).unwrap();
        // Switch #1 is marked as declared by a plugin.
        assert!(ir.symbols.switches.get(&1).unwrap().declared_by_plugin);
        // The banner asset is provided by a plugin.
        assert!(
            ir.plugin_provided_assets
                .contains(&AssetKey::new(AssetKind::Picture, "banner"))
        );
        // Metadata: only the enabled plugin in load order, the command is registered.
        assert_eq!(ir.plugin_meta.load_order, vec!["OwnsSwitch".to_string()]);
        assert_eq!(ir.plugin_meta.commands.len(), 1);
        assert!(ir.plugin_meta.is_present());

        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn build_plugin_db_param_ref_flagged_by_referential_integrity() {
        use dk_doctor_core::Msg;
        use dk_doctor_core::rules::referential_integrity::ReferentialIntegrity;
        use dk_doctor_core::{DbKind, Rule, RuleCtx, Severity};

        // A project where the plugin parameter `@type state` = 5, but States.json has only #1.
        let root = temp_root("plugin_db_ref");
        let data = root.join("data");
        std::fs::create_dir_all(data.as_std_path()).unwrap();
        std::fs::write(
            data.join("System.json").as_std_path(),
            br#"{"switches":["",""],"variables":["",""]}"#,
        )
        .unwrap();
        // Only state #1 exists → the reference to #5 is dangling.
        std::fs::write(
            data.join("States.json").as_std_path(),
            br#"[null,{"id":1,"name":"S1"}]"#,
        )
        .unwrap();

        let js = root.join("js");
        let pdir = js.join("plugins");
        std::fs::create_dir_all(pdir.as_std_path()).unwrap();
        std::fs::write(
            js.join("plugins.js").as_std_path(),
            r#"var $plugins =
[
{"name":"Immortalizer","status":true,"description":"","parameters":{"Immortal":"5"}}
];"#,
        )
        .unwrap();
        std::fs::write(
            pdir.join("Immortalizer.js").as_std_path(),
            r#"/*:
 * @param Immortal
 * @type state
 */"#,
        )
        .unwrap();

        let (ir, _warns) = build(&root).unwrap();
        let ctx = RuleCtx::new(&ir);
        let findings = ReferentialIntegrity.run(&ctx);
        // Exactly one dangling reference: state #5 from the plugin parameter.
        assert_eq!(findings.len(), 1);
        let f = &findings[0];
        assert_eq!(f.severity, Severity::Error);
        assert!(matches!(
            f.message,
            Msg::DanglingDbRef {
                kind: DbKind::State,
                id: 5
            }
        ));
        assert_eq!(f.location.file.as_str(), "js/plugins.js");
        assert_eq!(
            f.location.path.to_string(),
            "plugin:Immortalizer/param:Immortal"
        );

        let _ = std::fs::remove_dir_all(root.as_std_path());
    }
}
