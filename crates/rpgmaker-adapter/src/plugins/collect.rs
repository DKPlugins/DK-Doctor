//! Collecting Tier-A plugin facts into the IR.
//!
//! For each ENABLED plugin we read `js/plugins/<name>.js`, parse the header
//! ([`super::annotations`]) and combine the parameter schema with the values
//! from `plugins.js`:
//!  - `@type switch`/`variable` (+ `[]`) → ids owned by the plugin →
//!    `SymbolTable::mark_*_declared_by_plugin` (suppresses `uninitialized`);
//!  - `@type file` (+ `[]`) → assets provided by the plugin →
//!    `add_plugin_provided_asset` (suppresses `broken-assets`);
//!  - `@type <db>` (`common_event`/`state`/`actor`/… + `[]`) → DB record ids →
//!    `Edge::ReferencesDbId` on a lazy [`Entity::Plugin`] (catches
//!    `referential-integrity`; for `common_event` it also rescues `dead-common-event`);
//!  - **untyped** params (no `@type`, common on MV) → name-alias inference
//!    ([`annotations::infer_symbol_from_name`]): a `…Switch`/`…Variable` name →
//!    `mark_*_declared_by_plugin`, a `…Common Event` name → `add_reserved_common_event`
//!    (rescues `dead-common-event`). Suppression only — no finding is emitted, so a
//!    misfire costs at most a missed diagnostic, never a false alarm;
//!  - `@command` → command registry; `@base`/`@orderAfter`/`@orderBefore` →
//!    order declarations — both go into [`PluginMeta`].
//!
//! A file that cannot be read/parsed is skipped with a warning.

use crate::plugins::annotations::{self, InferredKind, ParamType};
use crate::plugins::js;
use crate::raw::plugins::PluginEntry;
use camino::Utf8Path;
use dk_doctor_core::ir::{
    AssetKey, AssetKind, Edge, Entity, EntityId, IrBuilder, Location, MethodPatch, PathSeg,
    PluginCommand, PluginMeta, PluginOrderDeps, PluginRef,
};

/// Maps `@dir`/folder → [`AssetKind`] for `@type file` parameters.
pub(crate) fn folder_to_kind(dir: &str) -> Option<AssetKind> {
    let norm = dir.trim_matches('/').to_ascii_lowercase();
    // Strip the leading `img/` (`@dir img/pictures` and `@dir pictures` are both valid).
    let tail = norm.strip_prefix("img/").unwrap_or(&norm);
    Some(match tail {
        "faces" => AssetKind::Face,
        "characters" => AssetKind::Character,
        "pictures" => AssetKind::Picture,
        "parallaxes" => AssetKind::Parallax,
        "tilesets" => AssetKind::Tileset,
        "battlebacks1" => AssetKind::Battleback1,
        "battlebacks2" => AssetKind::Battleback2,
        "titles1" => AssetKind::Title1,
        "titles2" => AssetKind::Title2,
        "enemies" => AssetKind::Enemy,
        "sv_enemies" => AssetKind::SvEnemy,
        "sv_actors" => AssetKind::SvActor,
        "animations" => AssetKind::Animation,
        "effects" => AssetKind::Effect,
        "movies" => AssetKind::Movie,
        "audio/bgm" => AssetKind::Bgm,
        "audio/bgs" => AssetKind::Bgs,
        "audio/me" => AssetKind::Me,
        "audio/se" => AssetKind::Se,
        _ => return None,
    })
}

/// Decodes a string parameter value into a set of ids (for switch/variable/db).
///
/// Scalar: `"5"` → `[5]`. Array (JSON string): `'["10","11"]'` → `[10, 11]`.
/// Non-numeric/empty elements and `0` are skipped; the result is deduplicated
/// while preserving order (a duplicate id yields no duplicate edges/findings).
pub(crate) fn decode_ids(value: &str, is_array: bool) -> Vec<u32> {
    let mut out = Vec::new();
    if is_array {
        // The value is a JSON string-array of strings (sometimes nested). Decode
        // as tolerantly as possible.
        if let Ok(arr) = serde_json::from_str::<Vec<String>>(value) {
            for s in arr {
                push_id(&s, &mut out);
            }
        } else {
            // Fallback: not a JSON array of strings. Split on array separators and parse
            // each token as u32 via push_id — the sign is NOT stripped, so that
            // negative sentinels (`-1` = "none/off") don't turn into `1`.
            for tok in value.split([',', '[', ']', ' ', '\t', '\n', '\r']) {
                push_id(tok, &mut out);
            }
        }
    } else {
        push_id(value, &mut out);
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|id| seen.insert(*id));
    out
}

fn push_id(s: &str, out: &mut Vec<u32>) {
    if let Ok(id) = s.trim().parse::<u32>()
        && id != 0
    {
        out.push(id);
    }
}

/// Decodes a `@type file` value into a set of asset paths (without extension).
pub(crate) fn decode_files(value: &str, is_array: bool) -> Vec<String> {
    let mut out = Vec::new();
    if is_array {
        if let Ok(arr) = serde_json::from_str::<Vec<String>>(value) {
            out = arr;
        }
    } else if !value.trim().is_empty() {
        out.push(value.to_string());
    }
    out.into_iter()
        .map(|s| asset_name(&s))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Normalizes a `@type file` value into a "bare" asset name (as an AssetKey key):
/// stripping leading folders up to the last `/` is NOT needed — an asset name may
/// include a subfolder; we strip only the extension and bracket prefix, like the scanner does.
fn asset_name(value: &str) -> String {
    let v = value.trim();
    // Take the file name from the path as-is (with subfolders), normalizing the last
    // component by the asset-scanner rules.
    let (subdir, file) = match v.rfind('/') {
        Some(idx) => (&v[..idx], &v[idx + 1..]),
        None => ("", v),
    };
    let norm = crate::assets::normalize_filename(file);
    if norm.is_empty() {
        return String::new();
    }
    if subdir.is_empty() {
        norm
    } else {
        format!("{subdir}/{norm}")
    }
}

/// Collects Tier-A facts of all enabled plugins into the IR.
///
/// `plugins` — entries from `plugins.js` (in load order). `plugins_dir` — the
/// `js/plugins/` directory. Returns a populated [`PluginMeta`] (which it also sets
/// on the builder) and pushes warnings about unreadable files into `warns`.
pub fn collect(
    b: &mut IrBuilder,
    plugins: &[PluginEntry],
    plugins_dir: &Utf8Path,
    plugins_js: &Utf8Path,
    warns: &mut Vec<String>,
) {
    let mut meta = PluginMeta::new();

    // Disabled plugins: needed by missing-base, to distinguish "off" from "absent".
    for entry in plugins.iter().filter(|p| !p.status) {
        meta.disabled.push(entry.name.clone());
    }

    for entry in plugins.iter().filter(|p| p.status) {
        meta.load_order.push(entry.name.clone());

        let path = plugins_dir.join(format!("{}.js", entry.name));
        let src = match std::fs::read_to_string(path.as_std_path()) {
            Ok(s) => s,
            Err(_) => {
                warns.push(format!(
                    "плагин {}: файл js/plugins/{}.js не читается — пропущен",
                    entry.name, entry.name
                ));
                continue;
            }
        };
        let ann = annotations::parse(&src);

        // The plugin's location in plugins.js — shared by its entity and the edges it spawns.
        let plugin_loc = Location::new(
            plugins_js.to_path_buf(),
            vec![PathSeg::Plugin(entry.name.clone())],
        );
        // The plugin entity is created lazily — only if it has a DB ref.
        let mut plugin_entity: Option<EntityId> = None;

        // Parameters: switch/variable/file/<db> by their values in plugins.js.
        for param in &ann.params {
            let Some(value) = entry.parameters.get(&param.name) else {
                continue;
            };
            match param.ty {
                ParamType::Switch => {
                    for id in decode_ids(value, param.is_array) {
                        b.symbols_mut().mark_switch_declared_by_plugin(id);
                    }
                }
                ParamType::Variable => {
                    for id in decode_ids(value, param.is_array) {
                        b.symbols_mut().mark_variable_declared_by_plugin(id);
                    }
                }
                ParamType::File => {
                    let Some(kind) = param.dir.as_deref().and_then(folder_to_kind) else {
                        continue;
                    };
                    for name in decode_files(value, param.is_array) {
                        b.add_plugin_provided_asset(AssetKey::new(kind, name));
                    }
                }
                ParamType::Db(kind) => {
                    // A `@type <db>` parameter value is a DB record id (0 = "none",
                    // skipped by decode_ids). The edge catches `referential-integrity`
                    // (dangling reference) and `dead-common-event` (the CE counts as live).
                    let ids = decode_ids(value, param.is_array);
                    if !ids.is_empty() {
                        let from = *plugin_entity.get_or_insert_with(|| {
                            b.push_entity(
                                Entity::Plugin(PluginRef {
                                    name: entry.name.clone(),
                                }),
                                plugin_loc.clone(),
                            )
                        });
                        // The location points to the specific parameter in plugins.js —
                        // "bug + exact location".
                        let ref_loc = Location::new(
                            plugins_js.to_path_buf(),
                            vec![
                                PathSeg::Plugin(entry.name.clone()),
                                PathSeg::Param(param.name.clone()),
                            ],
                        );
                        for id in ids {
                            b.push_edge(from, Edge::ReferencesDbId { kind, id }, ref_loc.clone());
                        }
                    }
                }
                ParamType::Other => {
                    // No recognized `@type`. On MV the editor never writes one, so
                    // fall back to name-alias inference (suffix-only): a `…Switch`/
                    // `…Variable`/`…Common Event` parameter names a symbol/CE id.
                    // Skipped when an explicit (but unrecognized) `@type` is present
                    // — `@type boolean` on a `…Switch` param is a toggle, not an id.
                    //
                    // Inference only ever SUPPRESSES a false positive (marks the
                    // symbol/CE as plugin-managed); it emits no finding, so a wrong
                    // guess costs a missed diagnostic, never a false alarm. Ids are
                    // decoded list-tolerantly (scalar, JSON array, or comma/space
                    // list as seen in MV plugins).
                    if !param.has_explicit_type
                        && let Some(kind) = annotations::infer_symbol_from_name(&param.name)
                    {
                        for id in decode_ids(value, true) {
                            match kind {
                                InferredKind::Switch => {
                                    b.symbols_mut().mark_switch_declared_by_plugin(id);
                                }
                                InferredKind::Variable => {
                                    b.symbols_mut().mark_variable_declared_by_plugin(id);
                                }
                                InferredKind::CommonEvent => b.add_reserved_common_event(id),
                            }
                        }
                    }
                }
            }
        }

        // Command registry from annotations (@command).
        for cmd in &ann.commands {
            meta.commands.push(PluginCommand {
                plugin: entry.name.clone(),
                command: cmd.clone(),
            });
        }

        // Order declarations.
        let deps = PluginOrderDeps {
            plugin: entry.name.clone(),
            base: ann.base.clone(),
            order_after: ann.order_after.clone(),
            order_before: ann.order_before.clone(),
        };
        if !deps.is_empty() {
            meta.order_deps.push(deps);
        }

        // --- Tier B: AST heuristics over the same source ---
        let facts = js::analyze_plugin(&src);

        // Literal switch/var writes → the symbol is managed by the plugin at runtime
        // (suppresses uninitialized + stuck-autorun on the pages it enables).
        for id in facts.switch_writes {
            b.symbols_mut().mark_switch_set_by_plugin(id);
        }
        for id in facts.variable_writes {
            b.symbols_mut().mark_variable_set_by_plugin(id);
        }
        // Literal variable reads ($gameVariables.value(N)) → not dead even if
        // written only from data (suppresses dead-variables false positives).
        for id in facts.variable_reads {
            b.symbols_mut().mark_variable_read_by_plugin(id);
        }

        // A common event reserved by the plugin ($gameTemp.reserveCommonEvent(N)) —
        // rescues it from `dead-common-event`.
        for id in facts.reserved_common_events {
            b.add_reserved_common_event(id);
        }

        // Assets loaded at runtime with a literal name (ImageManager.load*/
        // AudioManager.play*) → plugin-managed: not broken, not orphan.
        for (kind, name) in facts.provided_assets {
            b.add_plugin_provided_asset(AssetKey::new(kind, name));
        }

        // registerCommand → extend the command registry (plugin name from the argument).
        for (plugin, command) in &facts.commands {
            let pc = PluginCommand {
                plugin: plugin.clone(),
                command: command.clone(),
            };
            if !meta.commands.contains(&pc) {
                meta.commands.push(pc);
            }
        }

        // We treat the command registry as COMPLETE (so a typo in a connected
        // plugin can be caught) if there are @command annotations OR all registerCommand
        // calls are resolvable and non-empty.
        let mut known: Vec<String> = Vec::new();
        if !ann.commands.is_empty() {
            known.push(entry.name.clone());
        }
        if facts.registers_any && facts.registry_complete && !facts.commands.is_empty() {
            for (name, _) in &facts.commands {
                if !known.contains(name) {
                    known.push(name.clone());
                }
            }
        }
        for n in known {
            if !meta.command_registry_known.contains(&n) {
                meta.command_registry_known.push(n);
            }
        }

        // core-method patches → input for the plugin-conflict rule.
        for (method, overwrites) in facts.patches {
            meta.patches.push(MethodPatch {
                method,
                plugin: entry.name.clone(),
                overwrites,
            });
        }
    }

    b.set_plugin_meta(meta);
}

#[cfg(test)]
mod tests {
    use super::*;
    use dk_doctor_core::ir::{Engine, Ir};
    use std::collections::BTreeMap;

    fn params(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn decode_ids_scalar_and_array() {
        assert_eq!(decode_ids("5", false), vec![5]);
        assert_eq!(decode_ids("0", false), Vec::<u32>::new());
        assert_eq!(decode_ids(r#"["10","11","0"]"#, true), vec![10, 11]);
        // Fallback extraction of numbers from a non-JSON array.
        assert_eq!(decode_ids("[7, 8]", true), vec![7, 8]);
        // The negative sentinel `-1` ("none") does NOT turn into `1` (fallback).
        assert_eq!(decode_ids("[-1,4]", true), vec![4]);
        assert_eq!(decode_ids(r#"["-1","4"]"#, true), vec![4]);
        // Dedup preserving order.
        assert_eq!(decode_ids(r#"["3","3","4"]"#, true), vec![3, 4]);
    }

    #[test]
    fn folder_mapping_handles_img_prefix_and_audio() {
        assert_eq!(folder_to_kind("img/pictures"), Some(AssetKind::Picture));
        assert_eq!(folder_to_kind("pictures"), Some(AssetKind::Picture));
        assert_eq!(folder_to_kind("audio/se"), Some(AssetKind::Se));
        assert_eq!(folder_to_kind("weird/folder"), None);
    }

    #[test]
    fn collect_marks_symbols_and_provides_assets_and_registry() {
        // Set up a temporary plugin directory with an annotation header.
        let tmp = std::env::temp_dir().join(format!("dkplugincollect{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let src = r#"/*:
 * @param Sw
 * @type switch
 * @param Vars
 * @type variable[]
 * @param Pic
 * @type file
 * @dir img/pictures
 * @command doThing
 */"#;
        std::fs::write(tmp.join("Demo.js"), src).unwrap();

        let entry = PluginEntry {
            name: "Demo".to_string(),
            status: true,
            description: String::new(),
            parameters: params(&[("Sw", "7"), ("Vars", r#"["3","4"]"#), ("Pic", "logo")]),
        };
        // Disabled plugin — ignored.
        let disabled = PluginEntry {
            name: "Off".to_string(),
            status: false,
            description: String::new(),
            parameters: BTreeMap::new(),
        };

        let mut b = Ir::builder(Engine::Mz);
        let mut warns = Vec::new();
        let dir = camino::Utf8PathBuf::from_path_buf(tmp.clone()).unwrap();
        let pjs = camino::Utf8Path::new("js/plugins.js");
        collect(&mut b, &[entry, disabled], &dir, pjs, &mut warns);
        let ir = b.finish();

        assert!(ir.symbols.switches.get(&7).unwrap().declared_by_plugin);
        assert!(ir.symbols.variables.get(&3).unwrap().declared_by_plugin);
        assert!(ir.symbols.variables.get(&4).unwrap().declared_by_plugin);
        assert!(
            ir.plugin_provided_assets
                .contains(&AssetKey::new(AssetKind::Picture, "logo"))
        );
        assert_eq!(ir.plugin_meta.load_order, vec!["Demo".to_string()]);
        assert_eq!(ir.plugin_meta.commands.len(), 1);
        assert_eq!(ir.plugin_meta.commands[0].command, "doThing");
        assert!(ir.plugin_meta.is_present());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn collect_emits_db_ref_edges_for_db_typed_params() {
        use dk_doctor_core::ir::{DbKind, Edge, Entity};

        let tmp = std::env::temp_dir().join(format!("dkplugindbref{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let src = r#"/*:
 * @param OnLangChange
 * @type common_event
 * @param Bosses
 * @type enemy[]
 * @param NoneCE
 * @type common_event
 */"#;
        std::fs::write(tmp.join("Loc.js"), src).unwrap();

        let entry = PluginEntry {
            name: "Loc".to_string(),
            status: true,
            description: String::new(),
            parameters: params(&[
                ("OnLangChange", "50"),
                ("Bosses", r#"["3","0"]"#),
                ("NoneCE", "0"), // 0 = "none" → no edge is emitted
            ]),
        };

        let mut b = Ir::builder(Engine::Mz);
        let mut warns = Vec::new();
        let dir = camino::Utf8PathBuf::from_path_buf(tmp.clone()).unwrap();
        let pjs = camino::Utf8Path::new("js/plugins.js");
        collect(&mut b, &[entry], &dir, pjs, &mut warns);
        let ir = b.finish();

        let db_refs: Vec<(DbKind, u32)> = ir
            .edges
            .iter()
            .filter_map(|r| match r.edge {
                Edge::ReferencesDbId { kind, id } => Some((kind, id)),
                _ => None,
            })
            .collect();
        assert!(db_refs.contains(&(DbKind::CommonEvent, 50)));
        assert!(db_refs.contains(&(DbKind::Enemy, 3)));
        // value 0 is skipped (no CE0 nor Enemy0).
        assert!(!db_refs.iter().any(|&(_, id)| id == 0));
        // All plugin edges point into plugins.js at a specific parameter.
        for r in &ir.edges {
            if matches!(r.edge, Edge::ReferencesDbId { .. }) {
                assert_eq!(r.location.file.as_str(), "js/plugins.js");
                assert!(r.location.path.to_string().starts_with("plugin:Loc/param:"));
            }
        }
        // The name of the offending parameter ends up in the location ("bug + location" precision).
        let ce_edge = ir.edges.iter().find(|r| {
            matches!(
                r.edge,
                Edge::ReferencesDbId {
                    kind: DbKind::CommonEvent,
                    ..
                }
            )
        });
        assert_eq!(
            ce_edge.unwrap().location.path.to_string(),
            "plugin:Loc/param:OnLangChange"
        );
        // Exactly one plugin entity is created (lazily, for the whole plugin).
        assert_eq!(
            ir.entities
                .iter()
                .filter(|n| matches!(n.kind, Entity::Plugin(_)))
                .count(),
            1
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn collect_infers_untyped_suffix_params() {
        // MV-style plugin: params carry NO @type. Name-alias inference must treat
        // `…Switch`/`…Variable` as declared symbols and `…Common Event` as reserved.
        let tmp = std::env::temp_dir().join(format!("dkplugininfer{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let src = r#"/*:
 * @param Battle Switch
 * @param Day Switches IDs
 * @param Managed Switches
 * @param Zero Switch
 * @param Carried Variable
 * @param Alert Common Event
 * @param Draw Switches
 * @type boolean
 * @param Message Switcher
 */"#;
        std::fs::write(tmp.join("Infer.js"), src).unwrap();

        let entry = PluginEntry {
            name: "Infer".to_string(),
            status: true,
            description: String::new(),
            parameters: params(&[
                ("Battle Switch", "7"),
                ("Day Switches IDs", "29,30"), // comma-separated MV id list
                ("Managed Switches", r#"["21","22"]"#), // JSON string-array
                ("Zero Switch", "0"),          // 0 = "none" → nothing marked
                ("Carried Variable", "3"),
                ("Alert Common Event", "5"),
                ("Draw Switches", "9"), // @type boolean → explicit type wins, no inference
                ("Message Switcher", "11"), // "Switcher" != token "Switch" → no match
            ]),
        };

        let mut b = Ir::builder(Engine::Mz);
        let mut warns = Vec::new();
        let dir = camino::Utf8PathBuf::from_path_buf(tmp.clone()).unwrap();
        let pjs = camino::Utf8Path::new("js/plugins.js");
        collect(&mut b, &[entry], &dir, pjs, &mut warns);
        let ir = b.finish();

        // Switch / variable → declared by plugin (suppresses uninitialized).
        // Covers scalar (7), comma list (29,30) and JSON array (21,22) decoding.
        assert!(ir.symbols.switches.get(&7).unwrap().declared_by_plugin);
        assert!(ir.symbols.switches.get(&29).unwrap().declared_by_plugin);
        assert!(ir.symbols.switches.get(&30).unwrap().declared_by_plugin);
        assert!(ir.symbols.switches.get(&21).unwrap().declared_by_plugin);
        assert!(ir.symbols.switches.get(&22).unwrap().declared_by_plugin);
        assert!(ir.symbols.variables.get(&3).unwrap().declared_by_plugin);
        // Value 0 = "none" → no entry created.
        assert!(
            !ir.symbols.switches.contains_key(&0),
            "0 = none, не помечается"
        );
        // Common event → reserved (rescues dead-common-event).
        assert!(ir.reserved_common_events.contains(&5));
        // Negative controls: explicit @type boolean and "Switcher" are NOT inferred.
        assert!(
            !ir.symbols.switches.contains_key(&9),
            "@type boolean не выводится"
        );
        assert!(
            !ir.symbols.switches.contains_key(&11) && !ir.symbols.variables.contains_key(&11),
            "'Switcher' — не суффикс 'Switch'"
        );
        // Inference emits no DB-ref edges (pure suppression, no findings).
        assert!(
            !ir.edges
                .iter()
                .any(|r| matches!(r.edge, Edge::ReferencesDbId { .. })),
            "инференция не эмитит referential-integrity"
        );

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn collect_warns_on_unreadable_plugin_file() {
        let entry = PluginEntry {
            name: "Ghost".to_string(),
            status: true,
            description: String::new(),
            parameters: BTreeMap::new(),
        };
        let mut b = Ir::builder(Engine::Mz);
        let mut warns = Vec::new();
        let dir = camino::Utf8PathBuf::from("D:/nonexistent_dk_doctor_dir/plugins");
        let pjs = camino::Utf8Path::new("js/plugins.js");
        collect(&mut b, &[entry], &dir, pjs, &mut warns);
        let ir = b.finish();
        // The plugin is still in load_order, but the file wasn't read → warning.
        assert_eq!(ir.plugin_meta.load_order, vec!["Ghost".to_string()]);
        assert_eq!(warns.len(), 1);
        assert!(warns[0].contains("Ghost"));
    }
}
