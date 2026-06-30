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

use camino::Utf8Path;
use dk_doctor_core::ir::{AssetKey, AssetKind, IrBuilder};

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
/// order. Asset facts (`asset_roots`/`provided_subdir`) apply only to ENABLED
/// plugins; `map_param` applies to any that declare it (the parameter value is the
/// author's explicit declaration, and `unreachable-maps` is INFO). Writes the
/// suppressions into existing hooks (`plugin_provided_assets`/`assets_present`/`plugin_referenced_maps`).
pub fn apply(
    b: &mut IrBuilder,
    project_root: &Utf8Path,
    plugins: &[(String, std::collections::BTreeMap<String, String>, bool)],
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
        apply(&mut b, &root, &[], &mut warns);
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
        apply(&mut b, &root, &[], &mut warns);
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
        apply(&mut b, &root, &[], &mut warns);
        let ir = b.finish();
        assert!(ir.plugin_provided_assets.is_empty());
    }
}
