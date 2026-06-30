//! Asset scanner: walks `img/`, `audio/`, `effects/`, `movies/` and builds
//! the set of present files `assets_present`.
//!
//! The name is normalized by stripping the known extension and the encryption
//! suffix (MV: `.rpgmvp/.rpgmvo/.rpgmvm`; MZ: trailing `_`). The folder
//! determines the [`AssetKind`]. The files themselves are not read — only the
//! names are needed.

use dk_doctor_core::ir::{AssetKey, AssetKind};

/// Mapping of a subfolder to an asset kind (relative to the project root).
const IMG_FOLDERS: &[(&str, AssetKind)] = &[
    ("img/faces", AssetKind::Face),
    ("img/characters", AssetKind::Character),
    ("img/pictures", AssetKind::Picture),
    ("img/parallaxes", AssetKind::Parallax),
    ("img/tilesets", AssetKind::Tileset),
    ("img/battlebacks1", AssetKind::Battleback1),
    ("img/battlebacks2", AssetKind::Battleback2),
    ("img/titles1", AssetKind::Title1),
    ("img/titles2", AssetKind::Title2),
    ("img/enemies", AssetKind::Enemy),
    ("img/sv_enemies", AssetKind::SvEnemy),
    ("img/sv_actors", AssetKind::SvActor),
    ("img/animations", AssetKind::Animation),
];

const AUDIO_FOLDERS: &[(&str, AssetKind)] = &[
    ("audio/bgm", AssetKind::Bgm),
    ("audio/bgs", AssetKind::Bgs),
    ("audio/me", AssetKind::Me),
    ("audio/se", AssetKind::Se),
];

/// Strips the known extension and the encryption suffix from a file name.
///
/// Returns the bare asset name (as the reference specifies it). If the
/// extension is not one of the known ones — returns the stem as is.
pub fn normalize_filename(file_name: &str) -> String {
    let mut name = file_name.to_string();
    // MZ: trailing '_' (encrypted) — strip it first.
    if let Some(stripped) = name.strip_suffix('_') {
        name = stripped.to_string();
    }
    // Strip known extensions (MV encryption changes the entire extension).
    const EXTS: &[&str] = &[
        ".png", ".ogg", ".m4a", ".rpgmvp", ".rpgmvo", ".rpgmvm", ".webm", ".mp4", ".efkefc",
    ];
    let lower = name.to_ascii_lowercase();
    for ext in EXTS {
        if lower.ends_with(ext) {
            let cut = name.len() - ext.len();
            name.truncate(cut);
            break;
        }
    }
    strip_bracket_prefix(&name).to_string()
}

/// Strips the leading metadata tag in brackets `[…]` that some plugins add
/// (e.g. DKPlugins: `[frameset,2,2]DKPlugins`). The reference in the data
/// itself uses the bare name, so we remove the prefix for matching. Leading
/// `$`/`!` are genuine engine file-name characters and are NOT touched.
pub fn strip_bracket_prefix(name: &str) -> &str {
    if name.starts_with('[')
        && let Some(end) = name.find(']')
    {
        return &name[end + 1..];
    }
    name
}

/// Scans all asset folders under `root` and returns the set of keys.
pub fn scan(root: &camino::Utf8Path) -> Vec<AssetKey> {
    let mut out = Vec::new();
    for (folder, kind) in IMG_FOLDERS.iter().chain(AUDIO_FOLDERS.iter()) {
        scan_folder(root, folder, *kind, &mut out);
    }
    // effects/ — Effekseer (MZ), top-level.
    scan_folder(root, "effects", AssetKind::Effect, &mut out);
    // movies/ — top-level.
    scan_folder(root, "movies", AssetKind::Movie, &mut out);
    out
}

fn scan_folder(root: &camino::Utf8Path, rel: &str, kind: AssetKind, out: &mut Vec<AssetKey>) {
    let base = root.join(rel);
    scan_dir(&base, &base, kind, out);
}

/// Recursively walks an asset folder. The key name preserves the relative
/// subpath: in RPG Maker an asset name can contain a subfolder (`piski/guide` =
/// `img/pictures/piski/guide.png`), and the reference in the data uses exactly
/// that path — so a flat scan produced false broken-assets.
fn scan_dir(
    base: &camino::Utf8Path,
    dir: &camino::Utf8Path,
    kind: AssetKind,
    out: &mut Vec<AssetKey>,
) {
    let Ok(entries) = std::fs::read_dir(dir.as_std_path()) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        let Ok(child) = camino::Utf8PathBuf::from_path_buf(entry.path()) else {
            continue;
        };
        if ft.is_dir() {
            scan_dir(base, &child, kind, out);
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        let Ok(rel_path) = child.strip_prefix(base) else {
            continue;
        };
        let rel_str = rel_path.as_str().replace('\\', "/");
        let (subdir, file_name) = match rel_str.rfind('/') {
            Some(idx) => (&rel_str[..idx], &rel_str[idx + 1..]),
            None => ("", rel_str.as_str()),
        };
        let norm = normalize_filename(file_name);
        if norm.is_empty() {
            continue;
        }
        let name = if subdir.is_empty() {
            norm
        } else {
            format!("{subdir}/{norm}")
        };
        out.push(AssetKey::new(kind, name));
    }
}
