//! Plugin profiles + project `.dk-doctor` — the curated top tier.
//!
//! The most valuable fact in practice (from mining real plugins) is **asset_roots**:
//! a plugin loads assets from extra/runtime folders (localization
//! `img/<folder>/<locale>/`, HD packs, swappers), which makes naive rules falsely
//! flag both base references (as broken) and localized variants (as orphans).
//! A profile declares these roots → we suppress the false positives.
//!
//! Source of facts (by override priority): built-in DB (`include_str!`,
//! e.g. `DKTools_Localization`) → user `<project>/.dk-doctor/plugins/
//! <name>.toml` (override the built-ins) → project `<project>/.dk-doctor/
//! config.toml` (ignore-globs/extra roots for the long tail and "optional"
//! namespaces, e.g. unvoiced voice lines).
//!
//! Facts are folded into the ALREADY existing IR hooks (`plugin_provided_assets` /
//! `assets_present`), which `broken-assets` already skips — the core stays untouched.
//!
//! Beyond assets, a profile can declare **curated per-plugin parameter/command/
//! dependency facts** — the manual override for what the agnostic name-alias
//! inference (Tier A) cannot reach: `[[symbol_param]]` (value is switch/variable
//! ids → `declared_by_plugin`), `[[db_param]]`/`[[common_event_param]]` (value is
//! a DB record id → `ReferencesDbId`, `certain` because the profile is authored),
//! `[[asset_param]]` (value is an asset path → plugin-provided), `[[asset_pattern]]`
//! (a glob of runtime-loaded assets → not orphans, quality-gated against
//! whole-folder over-suppression), `[[plugin_command]]` (a dynamically registered
//! command → command registry) and `[dependency]` (`@base`/order overrides).
//! Quality-gate principle: a profile ADDS facts, it never blanket-silences a rule.

use crate::plugins::collect::{decode_files, decode_ids, folder_to_kind};
use camino::Utf8Path;
use dk_doctor_core::ir::{
    AssetKey, AssetKind, DbKind, Edge, Entity, EntityId, IrBuilder, Location, PathSeg,
    PluginCommand, PluginOrderDeps, PluginRef,
};

/// Built-in profiles (compiled in). File name == plugin `name`.
const BUILTIN_PROFILES: &[&str] = &[
    include_str!("../profiles/DKTools_Localization.toml"),
    include_str!("../profiles/DK_Message_Busts.toml"),
    include_str!("../profiles/DK_Event_Factory.toml"),
    include_str!("../profiles/DK_Picture_Choices.toml"),
    include_str!("../profiles/DK_Animated_Icons.toml"),
];

/// A profile's asset root.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct AssetRoot {
    /// The plugin loads **localized** copies from per-locale subfolders
    /// (`img/<folder>/<locale>/<name>`): a base reference is considered satisfied
    /// if a file with the same name exists on disk in any subfolder of the same kind.
    localized: bool,
}

/// A plugin-managed asset subfolder whose path is taken from a `plugins.js`
/// parameter. Files inside it are loaded by the plugin at runtime by name
/// (invisible to static analysis), so they are not counted as orphans.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct ProvidedSubdir {
    /// Standard kind-folder label (as in [`folder_of`]), e.g. `img/pictures`.
    kind: String,
    /// Name of the `plugins.js` parameter carrying the subfolder path (e.g. `bustsFolder`).
    param: String,
    /// Default folder if the parameter is missing/empty (e.g. `img/pictures/`).
    default: String,
}

/// A plugin parameter (`@type number[]`) whose values are map ids that the
/// plugin uses as a source (e.g. template-event maps of DK_Event_Factory).
/// The player never visits such maps → `unreachable-maps` must not flag them.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct MapParam {
    /// Name of the `plugins.js` parameter (e.g. `templates`).
    param: String,
}

/// A plugin parameter whose value is switch/variable id(s). The plugin manages
/// the symbol at runtime → `declared_by_plugin` (suppresses `uninitialized-symbols`).
/// Manual override for names the agnostic suffix inference does not reach.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct SymbolParam {
    /// Name of the `plugins.js` parameter.
    param: String,
    /// `switch` or `variable`.
    kind: String,
    /// Whether the value is an id array (JSON string-array or comma/space list).
    array: bool,
}

/// A plugin parameter whose value is DB record id(s) of a given kind → emits a
/// `ReferencesDbId` edge (`referential-integrity`; for `common_event` also rescues
/// `dead-common-event`). `certain` — a curated profile is an authored declaration.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct DbParam {
    /// Name of the `plugins.js` parameter.
    param: String,
    /// Record kind (`actor`/`item`/`state`/`common_event`/… — as in `@type <db>`).
    kind: String,
    /// Whether the value is an id array.
    array: bool,
}

/// Convenience form of [`DbParam`] fixed to `kind = common_event`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct CommonEventParam {
    /// Name of the `plugins.js` parameter.
    param: String,
    /// Whether the value is an id array.
    array: bool,
}

/// A plugin parameter whose value is asset path(s) loaded at runtime → marked
/// plugin-provided (not broken, not orphan).
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct AssetParam {
    /// Name of the `plugins.js` parameter.
    param: String,
    /// Standard kind-folder label (as in [`folder_to_kind`]), e.g. `img/pictures`.
    kind: String,
    /// Whether the value is a path array.
    array: bool,
}

/// A glob of assets the plugin loads by name at runtime (busts, HD packs). Present
/// files matching it are plugin-provided, not orphans. Quality-gated against
/// whole-folder over-suppression (see [`is_overbroad_glob`]).
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct AssetPattern {
    /// Glob over `folder/name` (e.g. `img/pictures/busts/*`).
    glob: String,
}

/// A command the plugin registers dynamically (invisible to Tier A/B annotation
/// parsing). Declaring it lets `unknown-plugin-command` resolve calls to it.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct PluginCommandDecl {
    /// Command name (matches the 356/357 command token).
    command: String,
}

/// Load-order overrides for a plugin that omits `@base`/`@orderAfter`/`@orderBefore`
/// in its header.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct DependencyDecl {
    /// `@base`-equivalent hard dependencies.
    base: Vec<String>,
    /// `@orderAfter`-equivalent.
    order_after: Vec<String>,
    /// `@orderBefore`-equivalent.
    order_before: Vec<String>,
}

impl DependencyDecl {
    fn is_empty(&self) -> bool {
        self.base.is_empty() && self.order_after.is_empty() && self.order_before.is_empty()
    }
}

/// Profile of a single plugin (TOML).
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct Profile {
    /// Plugin name (== `$plugins[].name` == the `.js` name).
    name: String,
    /// Target engine (`MV`/`MZ`/`both`) — informational for now.
    #[allow(dead_code)]
    target: String,
    /// Declared asset roots.
    asset_roots: Vec<AssetRoot>,
    /// Plugin-managed subfolders (path from a `plugins.js` parameter).
    /// The TOML key is `[[provided_subdir]]` (singular).
    #[serde(rename = "provided_subdir")]
    provided_subdirs: Vec<ProvidedSubdir>,
    /// Parameters whose values are ids of maps used by the plugin.
    /// The TOML key is `[[map_param]]` (singular).
    #[serde(rename = "map_param")]
    map_params: Vec<MapParam>,
    /// Parameters whose values are switch/variable ids (TOML `[[symbol_param]]`).
    #[serde(rename = "symbol_param")]
    symbol_params: Vec<SymbolParam>,
    /// Parameters whose values are DB record ids (TOML `[[db_param]]`).
    #[serde(rename = "db_param")]
    db_params: Vec<DbParam>,
    /// Parameters whose values are common-event ids (TOML `[[common_event_param]]`).
    #[serde(rename = "common_event_param")]
    common_event_params: Vec<CommonEventParam>,
    /// Parameters whose values are asset paths (TOML `[[asset_param]]`).
    #[serde(rename = "asset_param")]
    asset_params: Vec<AssetParam>,
    /// Globs of runtime-loaded assets (TOML `[[asset_pattern]]`).
    #[serde(rename = "asset_pattern")]
    asset_patterns: Vec<AssetPattern>,
    /// Dynamically registered commands (TOML `[[plugin_command]]`).
    #[serde(rename = "plugin_command")]
    plugin_commands: Vec<PluginCommandDecl>,
    /// Load-order overrides (TOML `[dependency]`).
    dependency: DependencyDecl,
}

/// Decodes a `@type number[]` value (a JSON string-array) into a set of map ids.
/// Tolerant: tries an array of strings, then an array of numbers; skips 0/non-numeric.
fn decode_map_ids(value: &str) -> Vec<u32> {
    if let Ok(arr) = serde_json::from_str::<Vec<String>>(value) {
        return arr
            .iter()
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .filter(|&n| n != 0)
            .collect();
    }
    if let Ok(arr) = serde_json::from_str::<Vec<i64>>(value) {
        return arr.iter().filter(|&&n| n > 0).map(|&n| n as u32).collect();
    }
    Vec::new()
}

/// Maps a `db_param.kind` string to a [`DbKind`] (same vocabulary as `@type <db>`).
fn db_kind_from_str(s: &str) -> Option<DbKind> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "actor" => DbKind::Actor,
        "class" => DbKind::Class,
        "skill" => DbKind::Skill,
        "item" => DbKind::Item,
        "weapon" => DbKind::Weapon,
        "armor" => DbKind::Armor,
        "enemy" => DbKind::Enemy,
        "troop" => DbKind::Troop,
        "state" => DbKind::State,
        "animation" => DbKind::Animation,
        "tileset" => DbKind::Tileset,
        "common_event" | "commonevent" => DbKind::CommonEvent,
        _ => return None,
    })
}

/// Quality-gate for `[[asset_pattern]]`: rejects globs that would suppress an
/// entire standard asset folder. A bare wildcard tail (`*`/`**`) is allowed only
/// once the glob has drilled into a subfolder (≥4 path segments, e.g.
/// `img/pictures/busts/*`); `img/pictures/*` or `audio/se/**` are over-broad.
/// A glob with a non-wildcard final segment (`img/pictures/logo`, `audio/se/a*_*`)
/// is always specific enough.
fn is_overbroad_glob(glob: &str) -> bool {
    let g = glob.trim();
    let segs: Vec<&str> = g.split('/').filter(|s| !s.is_empty()).collect();
    let Some(last) = segs.last() else {
        return true; // empty
    };
    let pure_wildcard = *last == "*" || *last == "**";
    pure_wildcard && segs.len() <= 3
}

/// Project config `.dk-doctor/config.toml`.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
struct DkConfig {
    /// Globs of asset references (form `folder/name`, e.g. `audio/se/a*_*`) that
    /// must NOT be counted as broken. For "optional" namespaces: unvoiced voice
    /// lines, placeholders, content packs — declared deliberately by the developer.
    ignore_assets: Vec<String>,
    /// The project uses localized asset subfolders (as in [`AssetRoot`]),
    /// even if the localizer plugin is not recognized by a profile.
    localized_assets: bool,
}

/// Minimal glob matcher: `*` — any sequence (including `/`), `?` —
/// exactly one character. The classic two-pointer algorithm with backtracking on `*`.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut mark = 0usize;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Reads a TOML file into `T`, returning `None` on error (with a warning).
fn read_toml<T: serde::de::DeserializeOwned>(
    path: &Utf8Path,
    what: &str,
    warns: &mut Vec<String>,
) -> Option<T> {
    let text = std::fs::read_to_string(path.as_std_path()).ok()?;
    match toml::from_str::<T>(&text) {
        Ok(v) => Some(v),
        Err(e) => {
            warns.push(format!("{what} {path}: ошибка TOML — {e}"));
            None
        }
    }
}

/// Collects profiles: built-in + user (override by name).
fn load_profiles(project_root: &Utf8Path, warns: &mut Vec<String>) -> Vec<Profile> {
    let mut by_name: std::collections::HashMap<String, Profile> = std::collections::HashMap::new();
    for src in BUILTIN_PROFILES {
        if let Ok(p) = toml::from_str::<Profile>(src)
            && !p.name.is_empty()
        {
            by_name.insert(p.name.clone(), p);
        }
    }
    let user_dir = project_root.join(".dk-doctor").join("plugins");
    if let Ok(entries) = std::fs::read_dir(user_dir.as_std_path()) {
        for entry in entries.flatten() {
            let Ok(path) = camino::Utf8PathBuf::from_path_buf(entry.path()) else {
                continue;
            };
            if path.extension() != Some("toml") {
                continue;
            }
            if let Some(p) = read_toml::<Profile>(&path, "профиль", warns)
                && !p.name.is_empty()
            {
                by_name.insert(p.name.clone(), p); // user override wins
            }
        }
    }
    by_name.into_values().collect()
}

/// Applies profiles and `.dk-doctor` to the facts collected in the builder.
///
/// `plugins` — ALL plugins (name + `parameters` values + whether enabled), in load
/// order. `plugins_js` — path of `plugins.js` relative to the project base (for
/// profile-emitted edge locations). Asset facts (`asset_roots`/`provided_subdir`)
/// and the curated param/command/dependency tables apply only to ENABLED plugins
/// (they describe runtime behavior); `map_param` applies to any that declare it
/// (the parameter value is the author's explicit declaration, and `unreachable-maps`
/// is INFO). Writes the facts into existing hooks (`plugin_provided_assets`/
/// `assets_present`/`plugin_referenced_maps`/symbols/edges/`plugin_meta`).
pub fn apply(
    b: &mut IrBuilder,
    project_root: &Utf8Path,
    plugins_js: &Utf8Path,
    plugins: &[crate::plugins::PluginParams],
    warns: &mut Vec<String>,
) {
    let config: DkConfig = read_toml(
        &project_root.join(".dk-doctor").join("config.toml"),
        "config",
        warns,
    )
    .unwrap_or_default();
    let profiles = load_profiles(project_root, warns);

    // Localization is active if .dk-doctor enables it OR an enabled plugin's
    // profile declares a localized root.
    let localized = config.localized_assets
        || profiles.iter().any(|p| {
            plugins.iter().any(|(n, _, en)| *en && n == &p.name)
                && p.asset_roots.iter().any(|r| r.localized)
        });

    // (1) ignore-globs → references that must not be counted as broken.
    let mut ignored: Vec<AssetKey> = Vec::new();
    if !config.ignore_assets.is_empty() {
        for (key, _) in b.asset_refs() {
            let full = format!("{}/{}", key.kind.folder(), key.name);
            if config.ignore_assets.iter().any(|g| glob_match(g, &full)) {
                ignored.push(key.clone());
            }
        }
    }

    // (2) localized: a base reference is satisfied if a file with the same name
    //     exists in any subfolder of the same kind (`<locale>/<name>`).
    let mut satisfied: Vec<AssetKey> = Vec::new();
    if localized {
        let present = b.assets_present();
        for (key, _) in b.asset_refs() {
            if key.name.contains('/') || present.contains(key) {
                continue; // already has a subfolder or is present at base
            }
            let suffix = format!("/{}", key.name);
            let has_variant = present
                .iter()
                .any(|p| p.kind == key.kind && p.name.ends_with(&suffix));
            if has_variant {
                satisfied.push(key.clone());
            }
        }
    }

    // (3) provided_subdirs: the plugin loads assets from a configurable subfolder
    //     by name (busts, etc.). Files inside it are plugin-managed, not orphans.
    let mut provided: Vec<AssetKey> = Vec::new();
    for (name, params, enabled) in plugins {
        if !enabled {
            continue; // only an enabled plugin loads assets
        }
        let Some(profile) = profiles.iter().find(|p| &p.name == name) else {
            continue;
        };
        for sub in &profile.provided_subdirs {
            let Some(kind) = AssetKind::from_folder(&sub.kind) else {
                continue; // non-standard/unknown kind → skip
            };
            let prefix = kind.folder();
            // Path from the parameter (if non-empty) or the profile default.
            let folder = params
                .get(&sub.param)
                .filter(|v| !v.trim().is_empty())
                .unwrap_or(&sub.default);
            let folder = folder.trim_matches('/');
            // The folder must lie inside the standard root of its kind.
            let Some(rest) = folder.strip_prefix(&format!("{prefix}/")) else {
                continue;
            };
            let subdir = rest.trim_matches('/');
            if subdir.is_empty() {
                // A default like `img/pictures/` — otherwise we'd suppress ALL pictures.
                continue;
            }
            let needle = format!("{subdir}/");
            for key in b.assets_present() {
                if key.kind == kind && key.name.starts_with(&needle) {
                    provided.push(key.clone());
                }
            }
        }
    }

    // (4) map_param: a plugin parameter (`@type number[]`) lists map ids that the
    //     plugin uses as a source (template-event maps). We mark them as referenced
    //     → `unreachable-maps` won't flag them. Applies even to DISABLED plugins:
    //     the parameter value is the author's explicit declaration.
    for (name, params, _enabled) in plugins {
        let Some(profile) = profiles.iter().find(|p| &p.name == name) else {
            continue;
        };
        for mp in &profile.map_params {
            if let Some(value) = params.get(&mp.param) {
                for id in decode_map_ids(value) {
                    b.add_plugin_referenced_map(id);
                }
            }
        }
    }

    for key in ignored {
        b.add_plugin_provided_asset(key);
    }
    for key in provided {
        b.add_plugin_provided_asset(key);
    }
    for key in satisfied {
        b.add_asset_present(key);
    }

    // (5) Curated per-plugin param/command/dependency tables — the manual override
    //     for what name-alias inference (Tier A) does not reach. These describe the
    //     plugin's RUNTIME behavior, so they apply only to ENABLED plugins.
    apply_curated_tables(b, plugins_js, plugins, &profiles, warns);
}

/// Applies the curated `[[symbol_param]]`/`[[db_param]]`/`[[common_event_param]]`/
/// `[[asset_param]]`/`[[asset_pattern]]`/`[[plugin_command]]`/`[dependency]` tables
/// of enabled plugins. Split out of [`apply`] for readability.
fn apply_curated_tables(
    b: &mut IrBuilder,
    plugins_js: &Utf8Path,
    plugins: &[crate::plugins::PluginParams],
    profiles: &[Profile],
    warns: &mut Vec<String>,
) {
    for (name, params, enabled) in plugins {
        if !enabled {
            continue;
        }
        let Some(profile) = profiles.iter().find(|p| &p.name == name) else {
            continue;
        };
        let plugin_loc = Location::new(
            plugins_js.to_path_buf(),
            vec![PathSeg::Plugin(name.clone())],
        );
        let param_loc = |param: &str| {
            Location::new(
                plugins_js.to_path_buf(),
                vec![
                    PathSeg::Plugin(name.clone()),
                    PathSeg::Param(param.to_string()),
                ],
            )
        };
        // Plugin entity created lazily, only if a db/common-event ref is emitted.
        let mut plugin_entity: Option<EntityId> = None;

        // symbol_param → declared_by_plugin (suppress uninitialized-symbols).
        for sp in &profile.symbol_params {
            let Some(value) = params.get(&sp.param) else {
                continue;
            };
            match sp.kind.trim().to_ascii_lowercase().as_str() {
                "switch" => {
                    for id in decode_ids(value, sp.array) {
                        b.symbols_mut().mark_switch_declared_by_plugin(id);
                    }
                }
                "variable" => {
                    for id in decode_ids(value, sp.array) {
                        b.symbols_mut().mark_variable_declared_by_plugin(id);
                    }
                }
                other => warns.push(format!(
                    "профиль {name}: symbol_param «{}» — неизвестный kind «{other}» (ожидается switch/variable)",
                    sp.param
                )),
            }
        }

        // db_param + common_event_param → ReferencesDbId edges (certain).
        let mut db_refs: Vec<(&str, DbKind, bool)> = Vec::new();
        for dp in &profile.db_params {
            match db_kind_from_str(&dp.kind) {
                Some(kind) => db_refs.push((dp.param.as_str(), kind, dp.array)),
                None => warns.push(format!(
                    "профиль {name}: db_param «{}» — неизвестный kind «{}»",
                    dp.param, dp.kind
                )),
            }
        }
        for cp in &profile.common_event_params {
            db_refs.push((cp.param.as_str(), DbKind::CommonEvent, cp.array));
        }
        for (param, kind, array) in db_refs {
            let Some(value) = params.get(param) else {
                continue;
            };
            let ids = decode_ids(value, array);
            if ids.is_empty() {
                continue;
            }
            let from = *plugin_entity.get_or_insert_with(|| {
                b.push_entity(
                    Entity::Plugin(PluginRef { name: name.clone() }),
                    plugin_loc.clone(),
                )
            });
            let loc = param_loc(param);
            for id in ids {
                b.push_edge(from, Edge::ReferencesDbId { kind, id }, loc.clone());
            }
        }

        // asset_param → plugin-provided assets (not broken / not orphan).
        for ap in &profile.asset_params {
            let Some(value) = params.get(&ap.param) else {
                continue;
            };
            let Some(kind) = folder_to_kind(&ap.kind) else {
                warns.push(format!(
                    "профиль {name}: asset_param «{}» — неизвестная папка «{}»",
                    ap.param, ap.kind
                ));
                continue;
            };
            for nm in decode_files(value, ap.array) {
                b.add_plugin_provided_asset(AssetKey::new(kind, nm));
            }
        }

        // asset_pattern → present files matching the glob are plugin-provided
        // (not orphans). Quality-gated against whole-folder over-suppression.
        for pat in &profile.asset_patterns {
            if is_overbroad_glob(&pat.glob) {
                warns.push(format!(
                    "профиль {name}: asset_pattern «{}» слишком широкий (подавил бы целую папку) — пропущен",
                    pat.glob
                ));
                continue;
            }
            let matches: Vec<AssetKey> = b
                .assets_present()
                .iter()
                .filter(|k| glob_match(&pat.glob, &format!("{}/{}", k.kind.folder(), k.name)))
                .cloned()
                .collect();
            for key in matches {
                b.add_plugin_provided_asset(key);
            }
        }

        // plugin_command → command registry (resolve calls; enable typo detection).
        for pc in &profile.plugin_commands {
            let cmd = pc.command.trim();
            if cmd.is_empty() {
                continue;
            }
            let meta = b.plugin_meta_mut();
            let entry = PluginCommand {
                plugin: name.clone(),
                command: cmd.to_string(),
            };
            if !meta.commands.contains(&entry) {
                meta.commands.push(entry);
            }
            if !meta.command_registry_known.contains(name) {
                meta.command_registry_known.push(name.clone());
            }
        }

        // dependency → order declarations (merge into an existing record if any).
        if !profile.dependency.is_empty() {
            let dep = &profile.dependency;
            let meta = b.plugin_meta_mut();
            if let Some(existing) = meta.order_deps.iter_mut().find(|d| &d.plugin == name) {
                existing.base.extend(dep.base.iter().cloned());
                existing.order_after.extend(dep.order_after.iter().cloned());
                existing
                    .order_before
                    .extend(dep.order_before.iter().cloned());
            } else {
                meta.order_deps.push(PluginOrderDeps {
                    plugin: name.clone(),
                    base: dep.base.clone(),
                    order_after: dep.order_after.clone(),
                    order_before: dep.order_before.clone(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `plugins.js` path used for profile-emitted edge locations in tests.
    fn pjs() -> &'static Utf8Path {
        Utf8Path::new("js/plugins.js")
    }

    /// Writes a per-plugin profile TOML into `<root>/.dk-doctor/plugins/<name>.toml`.
    fn write_profile(root: &std::path::Path, name: &str, body: &str) {
        let dir = root.join(".dk-doctor").join("plugins");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{name}.toml")), body).unwrap();
    }

    /// A single plugin with multiple `param=value` pairs and an enabled status.
    fn plugin_multi(
        name: &str,
        pairs: &[(&str, &str)],
        enabled: bool,
    ) -> Vec<(String, std::collections::BTreeMap<String, String>, bool)> {
        let params = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        vec![(name.to_string(), params, enabled)]
    }

    #[test]
    fn glob_matches_voice_namespace() {
        assert!(glob_match("audio/se/a*_*", "audio/se/a1_daniel_0_1"));
        assert!(glob_match("audio/se/a*_*", "audio/se/a1_otec_48"));
        assert!(!glob_match("audio/se/a*_*", "audio/se/Move1"));
        assert!(!glob_match("audio/se/a*_*", "audio/bgm/a1_daniel_0_1"));
        // `?` — exactly one character (precisely the voice scheme a{act}_{char}_{line}).
        assert!(glob_match("audio/se/a?_*_*", "audio/se/a1_daniel_0_1"));
        assert!(!glob_match("audio/se/a?_*_*", "audio/se/attack")); // no two '_'
        // no wildcards — exact match
        assert!(glob_match("audio/se/Exact", "audio/se/Exact"));
        assert!(!glob_match("audio/se/Exact", "audio/se/Exacts"));
    }

    #[test]
    fn ignore_glob_suppresses_broken_ref() {
        use dk_doctor_core::ir::{Engine, Ir, Location};
        // Prepare a temporary project with .dk-doctor/config.toml.
        let root = std::env::temp_dir().join(format!("dkprof{}", std::process::id()));
        let cfg = root.join(".dk-doctor");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(
            cfg.join("config.toml"),
            "ignore_assets = [\"audio/se/a*_*\"]\n",
        )
        .unwrap();

        let mut b = Ir::builder(Engine::Mz);
        // Reference to a "broken" voice line + to a real missing asset (control).
        b.add_asset_ref(
            AssetKey::new(AssetKind::Se, "a1_daniel_0_1"),
            Location::file_only("data/Map017.json"),
        );
        b.add_asset_ref(
            AssetKey::new(AssetKind::Picture, "Ghost"),
            Location::file_only("data/Map001.json"),
        );
        let mut warns = Vec::new();
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();
        apply(&mut b, &root, pjs(), &[], &mut warns);
        let ir = b.finish();
        // voice is suppressed (in plugin_provided_assets), Ghost is not.
        assert!(
            ir.plugin_provided_assets
                .contains(&AssetKey::new(AssetKind::Se, "a1_daniel_0_1"))
        );
        assert!(
            !ir.plugin_provided_assets
                .contains(&AssetKey::new(AssetKind::Picture, "Ghost"))
        );
        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn localized_variant_satisfies_base_ref() {
        use dk_doctor_core::ir::{Engine, Ir, Location};
        let root = std::env::temp_dir().join(format!("dkprofloc{}", std::process::id()));
        let cfg = root.join(".dk-doctor");
        std::fs::create_dir_all(&cfg).unwrap();
        std::fs::write(cfg.join("config.toml"), "localized_assets = true\n").unwrap();

        let mut b = Ir::builder(Engine::Mz);
        // The base Title reference is absent, but a localized ru/Title exists.
        b.add_asset_ref(
            AssetKey::new(AssetKind::Picture, "Title"),
            Location::file_only("data/System.json"),
        );
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "ru/Title"));
        let mut warns = Vec::new();
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();
        apply(&mut b, &root, pjs(), &[], &mut warns);
        let ir = b.finish();
        // The Title base is now considered present.
        assert!(
            ir.assets_present
                .contains(&AssetKey::new(AssetKind::Picture, "Title"))
        );
        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    /// Helper: a single enabled plugin with one `param=value` pair.
    fn one_plugin(
        name: &str,
        param: &str,
        value: &str,
    ) -> Vec<(String, std::collections::BTreeMap<String, String>, bool)> {
        one_plugin_status(name, param, value, true)
    }

    /// Helper: a single plugin with one `param=value` pair and a given enabled status.
    fn one_plugin_status(
        name: &str,
        param: &str,
        value: &str,
        enabled: bool,
    ) -> Vec<(String, std::collections::BTreeMap<String, String>, bool)> {
        let mut params = std::collections::BTreeMap::new();
        params.insert(param.to_string(), value.to_string());
        vec![(name.to_string(), params, enabled)]
    }

    #[test]
    fn provided_subdir_suppresses_orphan_busts() {
        use dk_doctor_core::ir::{Engine, Ir};
        // Project without .dk-doctor → only the built-in profiles apply
        // (including DK_Message_Busts). bustsFolder points to a nested subfolder.
        let root = std::env::temp_dir().join(format!("dkbust{}", std::process::id()));
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();

        let mut b = Ir::builder(Engine::Mz);
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "busts/A"));
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "busts/B"));
        // Control: a regular picture at the root must NOT end up in provided.
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "Real"));
        let mut warns = Vec::new();
        apply(
            &mut b,
            &root,
            pjs(),
            &one_plugin("DK_Message_Busts", "bustsFolder", "img/pictures/busts/"),
            &mut warns,
        );
        let ir = b.finish();
        assert!(
            ir.plugin_provided_assets
                .contains(&AssetKey::new(AssetKind::Picture, "busts/A"))
        );
        assert!(
            ir.plugin_provided_assets
                .contains(&AssetKey::new(AssetKind::Picture, "busts/B"))
        );
        assert!(
            !ir.plugin_provided_assets
                .contains(&AssetKey::new(AssetKind::Picture, "Real"))
        );
    }

    #[test]
    fn picture_choices_profile_suppresses_orphan_subdir() {
        use dk_doctor_core::ir::{Engine, Ir};
        // Built-in profile DK_Picture_Choices: folder points to a subfolder.
        let root = std::env::temp_dir().join(format!("dkpcchoices{}", std::process::id()));
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();

        let mut b = Ir::builder(Engine::Mz);
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "choices/Yes"));
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "Normal")); // control
        let mut warns = Vec::new();
        apply(
            &mut b,
            &root,
            pjs(),
            &one_plugin("DK_Picture_Choices", "folder", "img/pictures/choices/"),
            &mut warns,
        );
        let ir = b.finish();
        assert!(
            ir.plugin_provided_assets
                .contains(&AssetKey::new(AssetKind::Picture, "choices/Yes"))
        );
        assert!(
            !ir.plugin_provided_assets
                .contains(&AssetKey::new(AssetKind::Picture, "Normal"))
        );
    }

    #[test]
    fn provided_subdir_default_folder_is_noop() {
        use dk_doctor_core::ir::{Engine, Ir};
        // bustsFolder = stock img/pictures/ → empty subfolder → suppress NOTHING
        // (otherwise we'd collapse the whole pictures folder).
        let root = std::env::temp_dir().join(format!("dkbustnoop{}", std::process::id()));
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();

        let mut b = Ir::builder(Engine::Mz);
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "busts/A"));
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "Real"));
        let mut warns = Vec::new();
        apply(
            &mut b,
            &root,
            pjs(),
            &one_plugin("DK_Message_Busts", "bustsFolder", "img/pictures/"),
            &mut warns,
        );
        let ir = b.finish();
        assert!(ir.plugin_provided_assets.is_empty());
    }

    #[test]
    fn map_param_marks_referenced_maps_even_when_disabled() {
        use dk_doctor_core::ir::{Engine, Ir};
        // Built-in profile DK_Event_Factory: templates = number[] of source maps.
        // Works even for a DISABLED plugin (the author's explicit declaration).
        let root = std::env::temp_dir().join(format!("dkmapparam{}", std::process::id()));
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();

        for enabled in [true, false] {
            let mut b = Ir::builder(Engine::Mz);
            let mut warns = Vec::new();
            apply(
                &mut b,
                &root,
                pjs(),
                &one_plugin_status("DK_Event_Factory", "templates", "[\"2\"]", enabled),
                &mut warns,
            );
            let ir = b.finish();
            assert!(
                ir.plugin_referenced_maps.contains(&2),
                "карта 2 должна быть помечена (enabled={enabled})"
            );
        }
    }

    #[test]
    fn provided_subdir_inactive_when_plugin_disabled() {
        use dk_doctor_core::ir::{Engine, Ir};
        // Plugin not in the enabled list → the subfolder is not suppressed.
        let root = std::env::temp_dir().join(format!("dkbustoff{}", std::process::id()));
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();

        let mut b = Ir::builder(Engine::Mz);
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "busts/A"));
        let mut warns = Vec::new();
        apply(&mut b, &root, pjs(), &[], &mut warns);
        let ir = b.finish();
        assert!(ir.plugin_provided_assets.is_empty());
    }

    #[test]
    fn overbroad_glob_gate() {
        // Whole-folder wildcards are rejected; drilled-in / literal globs pass.
        for g in ["img/pictures/*", "audio/se/**", "img/**", "movies/*", ""] {
            assert!(is_overbroad_glob(g), "{g:?} should be over-broad");
        }
        for g in [
            "img/pictures/busts/*",
            "img/pictures/logo",
            "audio/se/a*_*",
            "img/pictures/portraits/**",
        ] {
            assert!(!is_overbroad_glob(g), "{g:?} should be specific enough");
        }
    }

    #[test]
    fn symbol_param_marks_declared_and_warns_on_bad_kind() {
        use dk_doctor_core::ir::{Engine, Ir};
        let root = std::env::temp_dir().join(format!("dksymparam{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_profile(
            root.as_path(),
            "TimeCore",
            r#"name = "TimeCore"
[[symbol_param]]
param = "gaugeSwitch"
kind = "switch"
[[symbol_param]]
param = "counters"
kind = "variable"
array = true
[[symbol_param]]
param = "weird"
kind = "bogus"
"#,
        );
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();
        let mut b = Ir::builder(Engine::Mz);
        let mut warns = Vec::new();
        apply(
            &mut b,
            &root,
            pjs(),
            &plugin_multi(
                "TimeCore",
                &[
                    ("gaugeSwitch", "40"),
                    ("counters", r#"["12","13"]"#),
                    ("weird", "5"),
                ],
                true,
            ),
            &mut warns,
        );
        let ir = b.finish();
        assert!(ir.symbols.switches.get(&40).unwrap().declared_by_plugin);
        assert!(ir.symbols.variables.get(&12).unwrap().declared_by_plugin);
        assert!(ir.symbols.variables.get(&13).unwrap().declared_by_plugin);
        // Unknown kind → warned, and switch/var 5 not created.
        assert!(!ir.symbols.switches.contains_key(&5));
        assert!(!ir.symbols.variables.contains_key(&5));
        assert!(warns.iter().any(|w| w.contains("bogus")));
        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn db_and_common_event_params_emit_refs() {
        use dk_doctor_core::ir::{DbKind, Edge, Engine, Entity, Ir};
        let root = std::env::temp_dir().join(format!("dkdbparam{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_profile(
            root.as_path(),
            "Loc",
            r#"name = "Loc"
[[db_param]]
param = "reviveItem"
kind = "item"
[[common_event_param]]
param = "onStart"
"#,
        );
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();
        let mut b = Ir::builder(Engine::Mz);
        let mut warns = Vec::new();
        apply(
            &mut b,
            &root,
            pjs(),
            &plugin_multi("Loc", &[("reviveItem", "99"), ("onStart", "7")], true),
            &mut warns,
        );
        let ir = b.finish();
        let db_refs: Vec<(DbKind, u32)> = ir
            .edges
            .iter()
            .filter_map(|r| match r.edge {
                Edge::ReferencesDbId { kind, id } => Some((kind, id)),
                _ => None,
            })
            .collect();
        assert!(db_refs.contains(&(DbKind::Item, 99)));
        assert!(db_refs.contains(&(DbKind::CommonEvent, 7)));
        // Edge location points at the offending parameter in plugins.js.
        let ce_edge = ir
            .edges
            .iter()
            .find(|r| {
                matches!(
                    r.edge,
                    Edge::ReferencesDbId {
                        kind: DbKind::CommonEvent,
                        ..
                    }
                )
            })
            .unwrap();
        assert_eq!(
            ce_edge.location.path.to_string(),
            "plugin:Loc/param:onStart"
        );
        // A single lazy Plugin entity for both refs.
        assert_eq!(
            ir.entities
                .iter()
                .filter(|n| matches!(n.kind, Entity::Plugin(_)))
                .count(),
            1
        );
        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn asset_param_marks_provided() {
        use dk_doctor_core::ir::{Engine, Ir};
        let root = std::env::temp_dir().join(format!("dkassetparam{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_profile(
            root.as_path(),
            "Portraits",
            r#"name = "Portraits"
[[asset_param]]
param = "logo"
kind = "img/pictures"
"#,
        );
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();
        let mut b = Ir::builder(Engine::Mz);
        let mut warns = Vec::new();
        apply(
            &mut b,
            &root,
            pjs(),
            &plugin_multi("Portraits", &[("logo", "Splash")], true),
            &mut warns,
        );
        let ir = b.finish();
        assert!(
            ir.plugin_provided_assets
                .contains(&AssetKey::new(AssetKind::Picture, "Splash"))
        );
        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn asset_pattern_suppresses_present_and_skips_overbroad() {
        use dk_doctor_core::ir::{Engine, Ir};
        let root = std::env::temp_dir().join(format!("dkassetpat{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_profile(
            root.as_path(),
            "Busts",
            r#"name = "Busts"
[[asset_pattern]]
glob = "img/pictures/portraits/*"
[[asset_pattern]]
glob = "img/pictures/*"
"#,
        );
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();
        let mut b = Ir::builder(Engine::Mz);
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "portraits/Hero"));
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "Normal")); // control
        let mut warns = Vec::new();
        apply(
            &mut b,
            &root,
            pjs(),
            &plugin_multi("Busts", &[], true),
            &mut warns,
        );
        let ir = b.finish();
        // Drilled-in glob suppresses the subfolder file...
        assert!(
            ir.plugin_provided_assets
                .contains(&AssetKey::new(AssetKind::Picture, "portraits/Hero"))
        );
        // ...but the over-broad `img/pictures/*` is rejected (Normal stays an orphan candidate).
        assert!(
            !ir.plugin_provided_assets
                .contains(&AssetKey::new(AssetKind::Picture, "Normal"))
        );
        assert!(warns.iter().any(|w| w.contains("слишком широкий")));
        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn plugin_command_registers_and_marks_known() {
        use dk_doctor_core::ir::{Engine, Ir};
        let root = std::env::temp_dir().join(format!("dkplugincmd{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_profile(
            root.as_path(),
            "Spawner",
            r#"name = "Spawner"
[[plugin_command]]
command = "spawnEvent"
"#,
        );
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();
        let mut b = Ir::builder(Engine::Mz);
        let mut warns = Vec::new();
        apply(
            &mut b,
            &root,
            pjs(),
            &plugin_multi("Spawner", &[], true),
            &mut warns,
        );
        let ir = b.finish();
        assert!(
            ir.plugin_meta
                .commands
                .iter()
                .any(|c| c.plugin == "Spawner" && c.command == "spawnEvent")
        );
        assert!(
            ir.plugin_meta
                .command_registry_known
                .contains(&"Spawner".to_string())
        );
        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn dependency_adds_order_deps() {
        use dk_doctor_core::ir::{Engine, Ir};
        let root = std::env::temp_dir().join(format!("dkdep{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_profile(
            root.as_path(),
            "Dep",
            r#"name = "Dep"
[dependency]
base = ["CoreEngine"]
order_after = ["OtherPlugin"]
"#,
        );
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();
        let mut b = Ir::builder(Engine::Mz);
        let mut warns = Vec::new();
        apply(
            &mut b,
            &root,
            pjs(),
            &plugin_multi("Dep", &[], true),
            &mut warns,
        );
        let ir = b.finish();
        let deps = ir
            .plugin_meta
            .order_deps
            .iter()
            .find(|d| d.plugin == "Dep")
            .expect("order deps for Dep");
        assert_eq!(deps.base, vec!["CoreEngine".to_string()]);
        assert_eq!(deps.order_after, vec!["OtherPlugin".to_string()]);
        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn curated_tables_inactive_when_plugin_disabled() {
        use dk_doctor_core::ir::{Engine, Ir};
        let root = std::env::temp_dir().join(format!("dkcurateoff{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_profile(
            root.as_path(),
            "TimeCore",
            r#"name = "TimeCore"
[[symbol_param]]
param = "gaugeSwitch"
kind = "switch"
"#,
        );
        let root = camino::Utf8PathBuf::from_path_buf(root).unwrap();
        let mut b = Ir::builder(Engine::Mz);
        let mut warns = Vec::new();
        apply(
            &mut b,
            &root,
            pjs(),
            &plugin_multi("TimeCore", &[("gaugeSwitch", "40")], false), // disabled
            &mut warns,
        );
        let ir = b.finish();
        assert!(
            !ir.symbols.switches.contains_key(&40),
            "disabled plugin does not manage symbols at runtime"
        );
        let _ = std::fs::remove_dir_all(root.as_std_path());
    }

    #[test]
    fn builtin_profiles_pass_quality_gate() {
        // Every compiled-in profile must parse, be named, and declare only
        // resolvable kinds / non-over-broad globs (governance checklist).
        for src in BUILTIN_PROFILES {
            let p: Profile = toml::from_str(src).expect("built-in profile parses");
            assert!(!p.name.is_empty(), "built-in profile must have a name");
            for sp in &p.symbol_params {
                let k = sp.kind.trim().to_ascii_lowercase();
                assert!(k == "switch" || k == "variable", "symbol_param kind {k}");
            }
            for dp in &p.db_params {
                assert!(
                    db_kind_from_str(&dp.kind).is_some(),
                    "db_param kind {}",
                    dp.kind
                );
            }
            for ap in &p.asset_params {
                assert!(
                    folder_to_kind(&ap.kind).is_some(),
                    "asset_param kind {}",
                    ap.kind
                );
            }
            for pat in &p.asset_patterns {
                assert!(
                    !is_overbroad_glob(&pat.glob),
                    "over-broad glob {}",
                    pat.glob
                );
            }
        }
    }
}
