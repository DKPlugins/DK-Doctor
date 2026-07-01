//! Render-only map geometry sidecar for the desktop "Atlas" view.
//!
//! Intentionally SEPARATE from the analysis pipeline: the [`crate::Ir`] and the
//! findings JSON are never touched, and the contract test stays green. The
//! desktop re-reads map geometry on demand (map size + event tile coordinates)
//! purely for the spatial map view, so potentially large tile data never enters
//! the analysis IR. Only the few fields the schematic needs are deserialized.

use crate::build::detect_layout;
use crate::raw::map::MapInfo;
use camino::{Utf8Component, Utf8Path};
use serde::{Deserialize, Serialize};

/// One map's geometry plus its event placements (schematic, no tile art).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapAtlas {
    /// Map id (== `MapXXX` number).
    pub map_id: u32,
    /// Display name from `MapInfos.json` (may be empty).
    pub name: String,
    /// Parent map id from `MapInfos.json` (0 = root) — for a future map tree.
    pub parent_id: u32,
    /// Map width in tiles.
    pub width: u32,
    /// Map height in tiles.
    pub height: u32,
    /// Events placed on the grid.
    pub events: Vec<AtlasEvent>,
}

/// One event placed on the map grid.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AtlasEvent {
    /// Event id (== index in the map's `events[]`).
    pub id: u32,
    /// Event name (editor label, may be empty).
    pub name: String,
    /// Tile x coordinate.
    pub x: i32,
    /// Tile y coordinate.
    pub y: i32,
    /// First-page graphic (sprite or tile); `None` when the event is invisible.
    pub graphic: Option<EventGraphic>,
}

/// The graphic of an event's first page — enough to draw it in the Atlas.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventGraphic {
    /// Character sheet stem (`img/characters/<name>.png`); empty for a tile graphic.
    pub character_name: String,
    /// Index of the sub-character (0..7) in a normal 8-char sheet.
    pub character_index: u32,
    /// Facing direction (2=down, 4=left, 6=right, 8=up).
    pub direction: u32,
    /// Walk frame / pattern (0..2).
    pub pattern: u32,
    /// Tile id when the event is drawn as a tileset tile (>0), else 0.
    pub tile_id: u32,
}

/// Full render data for one map (Wave 2 — real tiles): geometry + the layered
/// tile-id array + the tileset image slot names. Still render-only and separate
/// from the analysis IR; read on demand when a map is opened.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapRender {
    /// Map width in tiles.
    pub width: u32,
    /// Map height in tiles.
    pub height: u32,
    /// Tileset id used by this map.
    pub tileset_id: u32,
    /// `tilesetNames[9]` = [A1,A2,A3,A4,A5,B,C,D,E] image stems (may be empty).
    pub tileset_names: Vec<String>,
    /// Flat layered tile-id array (length = width*height*6).
    pub data: Vec<i32>,
}

/// Render-only deserialize of `MapXXX.json` — only the schematic-relevant fields.
#[derive(Deserialize)]
struct RawMap {
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    events: Vec<Option<RawEvent>>,
}

/// Render-only deserialize of `MapXXX.json` for the full-tile renderer.
#[derive(Deserialize, Default)]
struct RawMapTiles {
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default, rename = "tilesetId")]
    tileset_id: u32,
    #[serde(default)]
    data: Vec<i32>,
}

/// Render-only deserialize of a `Tilesets.json` entry (image slot names only).
#[derive(Deserialize, Default)]
struct RawTileset {
    #[serde(default, rename = "tilesetNames")]
    tileset_names: Vec<String>,
}

#[derive(Deserialize)]
struct RawEvent {
    #[serde(default)]
    id: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    x: i32,
    #[serde(default)]
    y: i32,
    #[serde(default)]
    pages: Vec<RawPage>,
}

#[derive(Deserialize, Default)]
struct RawPage {
    #[serde(default)]
    image: RawImage,
}

#[derive(Deserialize, Default)]
struct RawImage {
    #[serde(default, rename = "characterName")]
    character_name: String,
    #[serde(default, rename = "characterIndex")]
    character_index: u32,
    #[serde(default)]
    direction: u32,
    #[serde(default)]
    pattern: u32,
    #[serde(default, rename = "tileId")]
    tile_id: u32,
}

/// Builds the map-atlas geometry sidecar for a project root.
///
/// Re-reads `MapInfos.json` (names + parents) and each `MapXXX.json` (size +
/// event coordinates). Tolerant to junk: unreadable or unparseable maps are
/// skipped rather than failing the whole sidecar.
pub fn map_atlas(root: &Utf8Path) -> Result<Vec<MapAtlas>, crate::AdapterError> {
    let Some(layout) = detect_layout(root) else {
        return Err(crate::AdapterError::ProjectNotFound(root.to_string()));
    };
    let data = layout.data_dir();

    // Names + parents from MapInfos (sparse array, index == id).
    let infos: Vec<Option<MapInfo>> =
        std::fs::read_to_string(data.join("MapInfos.json").as_std_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
    let mut meta: std::collections::HashMap<u32, (String, u32)> = std::collections::HashMap::new();
    let mut ids: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for info in infos.into_iter().flatten() {
        ids.insert(info.id);
        meta.insert(info.id, (info.name, info.parent_id));
    }
    // Also pick up MapNNN.json files not listed in MapInfos.
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

    let mut out = Vec::new();
    for id in ids {
        if id == 0 {
            continue;
        }
        let file = format!("Map{id:03}.json");
        let Ok(text) = std::fs::read_to_string(data.join(&file).as_std_path()) else {
            continue;
        };
        let Ok(m) = serde_json::from_str::<RawMap>(&text) else {
            continue;
        };
        let (name, parent_id) = meta.get(&id).cloned().unwrap_or_default();
        let events = m
            .events
            .into_iter()
            .flatten()
            .filter(|e| e.id != 0)
            .map(|e| {
                let graphic = e.pages.first().and_then(|p| {
                    let img = &p.image;
                    if img.character_name.is_empty() && img.tile_id == 0 {
                        None
                    } else {
                        Some(EventGraphic {
                            character_name: img.character_name.clone(),
                            character_index: img.character_index,
                            direction: img.direction,
                            pattern: img.pattern,
                            tile_id: img.tile_id,
                        })
                    }
                });
                AtlasEvent {
                    id: e.id,
                    name: e.name,
                    x: e.x,
                    y: e.y,
                    graphic,
                }
            })
            .collect();
        out.push(MapAtlas {
            map_id: id,
            name,
            parent_id,
            width: m.width,
            height: m.height,
            events,
        });
    }
    Ok(out)
}

/// Reads the full tile render data for one map: geometry + tile array + the
/// tileset image slot names. Render-only and on-demand (the analysis IR never
/// carries tile arrays). Errors if the project or map file is unreadable.
pub fn map_render(root: &Utf8Path, map_id: u32) -> Result<MapRender, crate::AdapterError> {
    let Some(layout) = detect_layout(root) else {
        return Err(crate::AdapterError::ProjectNotFound(root.to_string()));
    };
    let data_dir = layout.data_dir();

    let file = format!("Map{map_id:03}.json");
    let text = std::fs::read_to_string(data_dir.join(&file).as_std_path())
        .map_err(crate::AdapterError::Io)?;
    let m: RawMapTiles = serde_json::from_str(&text).map_err(|e| crate::AdapterError::Json {
        file: file.clone(),
        source: e,
    })?;

    // Tileset image slot names (sparse array, index == tileset id).
    let tileset_names = std::fs::read_to_string(data_dir.join("Tilesets.json").as_std_path())
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Option<RawTileset>>>(&t).ok())
        .and_then(|list| {
            list.into_iter()
                .nth(m.tileset_id as usize)
                .flatten()
                .map(|ts| ts.tileset_names)
        })
        .unwrap_or_default();

    Ok(MapRender {
        width: m.width,
        height: m.height,
        tileset_id: m.tileset_id,
        tileset_names,
        data: m.data,
    })
}

/// One command line of an event page, for the "context" view in the UI.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandLine {
    /// 0-based index in the page command list (matches the finding's `cmdN`).
    pub index: u32,
    /// Indent level (nesting depth).
    pub indent: i32,
    /// RPG Maker command code (named on the UI side).
    pub code: u32,
    /// Language-neutral key argument (switch/var/CE id, self-switch ch), or empty.
    pub arg: String,
}

/// Render-only deserialize of a map for the command-context view.
#[derive(Deserialize, Default)]
struct RawCmdMap {
    #[serde(default)]
    events: Vec<Option<RawCmdEvent>>,
}

#[derive(Deserialize)]
struct RawCmdEvent {
    #[serde(default)]
    id: u32,
    #[serde(default)]
    pages: Vec<RawCmdPage>,
}

#[derive(Deserialize, Default)]
struct RawCmdPage {
    #[serde(default)]
    list: Vec<RawCommand>,
}

#[derive(Deserialize)]
struct RawCommand {
    #[serde(default)]
    code: u32,
    #[serde(default)]
    indent: i32,
    #[serde(default)]
    parameters: Vec<serde_json::Value>,
}

/// Extracts a language-neutral key argument (an id or self-switch channel) for
/// the common command codes — enough to disambiguate a line in context.
fn command_arg(code: u32, p: &[serde_json::Value]) -> String {
    let num = |i: usize| p.get(i).and_then(serde_json::Value::as_i64);
    match code {
        121 | 122 | 117 => num(0).map(|v| format!("#{v:04}")).unwrap_or_default(), // switch/var/CE id
        123 => p
            .first()
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_default(), // self-switch ch
        201 => num(1).map(|v| format!("#{v:03}")).unwrap_or_default(), // transfer target map
        111 => match num(0) {
            Some(0) => num(1).map(|v| format!("sw#{v:04}")).unwrap_or_default(),
            Some(1) => num(1).map(|v| format!("var#{v:04}")).unwrap_or_default(),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// Returns the command list of one event page (1-based `page`) for the context
/// view. Render-only; the finding's `cmdN` indexes into the returned list.
pub fn event_page_commands(
    root: &Utf8Path,
    map_id: u32,
    event_id: u32,
    page: u32,
) -> Result<Vec<CommandLine>, crate::AdapterError> {
    let Some(layout) = detect_layout(root) else {
        return Err(crate::AdapterError::ProjectNotFound(root.to_string()));
    };
    let file = format!("Map{map_id:03}.json");
    let text = std::fs::read_to_string(layout.data_dir().join(&file).as_std_path())
        .map_err(crate::AdapterError::Io)?;
    let m: RawCmdMap =
        serde_json::from_str(&text).map_err(|e| crate::AdapterError::Json { file, source: e })?;

    let Some(event) = m.events.into_iter().flatten().find(|e| e.id == event_id) else {
        return Ok(Vec::new());
    };
    let page_idx = page.saturating_sub(1) as usize;
    let Some(p) = event.pages.into_iter().nth(page_idx) else {
        return Ok(Vec::new());
    };

    Ok(p.list
        .into_iter()
        .enumerate()
        .map(|(i, c)| CommandLine {
            index: i as u32,
            indent: c.indent,
            code: c.code,
            arg: command_arg(c.code, &c.parameters),
        })
        .collect())
}

/// Map-transition graph for the desktop "Map graph" view: nodes are maps, edges
/// are direct Transfer Player (201) commands between them. Render-only and
/// on-demand — the analysis IR is never touched (the `unreachable-maps` rule owns
/// the analytical claim; this is a spatial overview aid).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapGraph {
    /// Start map id from `System.json` (0 = unset).
    pub start_map_id: u32,
    /// Every map present in the project.
    pub nodes: Vec<MapNode>,
    /// Direct (statically resolved) transfer edges, deduplicated.
    pub edges: Vec<MapEdge>,
}

/// One map node in the transition graph.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapNode {
    /// Map id.
    pub id: u32,
    /// Display name (may be empty).
    pub name: String,
    /// Count of by-variable (dynamic) transfer exits not resolvable statically —
    /// so the UI can note the graph is not exhaustive for this map.
    pub dynamic_exits: u32,
}

/// A directed transfer edge (source map → target map).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapEdge {
    /// Source map id.
    pub from: u32,
    /// Target map id (may be absent from `nodes` → a broken transfer).
    pub to: u32,
}

/// Builds the map-transition graph sidecar for a project root.
///
/// Scans each `MapXXX.json` for Transfer Player (201) commands: direct targets
/// (`params[0]==0`) become edges; by-variable targets are counted per map as
/// `dynamic_exits` (their destination cannot be known statically). Tolerant to
/// junk — unreadable/unparseable maps are skipped.
pub fn map_graph(root: &Utf8Path) -> Result<MapGraph, crate::AdapterError> {
    let Some(layout) = detect_layout(root) else {
        return Err(crate::AdapterError::ProjectNotFound(root.to_string()));
    };
    let data = layout.data_dir();

    // Start map id (best-effort — a missing/garbled System.json means 0).
    let start_map_id = std::fs::read_to_string(data.join("System.json").as_std_path())
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("startMapId").and_then(serde_json::Value::as_u64))
        .unwrap_or(0) as u32;

    // Names + the authoritative set of existing map ids (MapInfos + MapNNN files).
    let infos: Vec<Option<MapInfo>> =
        std::fs::read_to_string(data.join("MapInfos.json").as_std_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
    let mut names: std::collections::HashMap<u32, String> = std::collections::HashMap::new();
    let mut ids: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    for info in infos.into_iter().flatten() {
        ids.insert(info.id);
        names.insert(info.id, info.name);
    }
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

    // Collect edges (deduped) + dynamic-exit counts by scanning each map.
    let mut edge_set: std::collections::BTreeSet<(u32, u32)> = std::collections::BTreeSet::new();
    let mut dynamic: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
    for &id in &ids {
        if id == 0 {
            continue;
        }
        let file = format!("Map{id:03}.json");
        let Ok(text) = std::fs::read_to_string(data.join(&file).as_std_path()) else {
            continue;
        };
        let Ok(m) = serde_json::from_str::<RawCmdMap>(&text) else {
            continue;
        };
        for ev in m.events.into_iter().flatten() {
            for page in ev.pages {
                for cmd in page.list {
                    if cmd.code != 201 {
                        continue;
                    }
                    let designation = cmd.parameters.first().and_then(serde_json::Value::as_i64);
                    let target = cmd.parameters.get(1).and_then(serde_json::Value::as_i64);
                    match designation {
                        Some(0) | None => {
                            if let Some(to) = target
                                && to > 0
                            {
                                edge_set.insert((id, to as u32));
                            }
                        }
                        _ => {
                            *dynamic.entry(id).or_default() += 1;
                        }
                    }
                }
            }
        }
    }

    let nodes = ids
        .iter()
        .filter(|&&id| id != 0)
        .map(|&id| MapNode {
            id,
            name: names.get(&id).cloned().unwrap_or_default(),
            dynamic_exits: dynamic.get(&id).copied().unwrap_or(0),
        })
        .collect();
    let edges = edge_set
        .into_iter()
        .map(|(from, to)| MapEdge { from, to })
        .collect();

    Ok(MapGraph {
        start_map_id,
        nodes,
        edges,
    })
}

/// Reads a project image file (e.g. `img/tilesets/World.png`) and returns its
/// raw bytes. The path is resolved relative to the project asset root (root or
/// `www/`). If the plain `.png` is absent, the RPG Maker encrypted variants
/// (`.rpgmvp` for MV, `.png_` for MZ) are read and decrypted with the project's
/// `encryptionKey`. Path traversal is rejected. Used by the Atlas tile renderer.
pub fn read_project_image(root: &Utf8Path, rel: &str) -> Result<Vec<u8>, crate::AdapterError> {
    // Reject path traversal and any absolute path. The string checks catch a
    // leading separator and `..`; the component scan additionally catches a
    // Windows drive prefix (`C:\…` or drive-relative `C:foo`), which
    // `base.join(rel)` would otherwise resolve to a location *outside* the
    // project, discarding `base` entirely and reading an arbitrary file.
    let rel_path = Utf8Path::new(rel);
    if rel.contains("..")
        || rel.starts_with('/')
        || rel.starts_with('\\')
        || rel_path.is_absolute()
        || rel_path
            .components()
            .any(|c| matches!(c, Utf8Component::Prefix(_) | Utf8Component::RootDir))
    {
        return Err(crate::AdapterError::ProjectNotFound(rel.to_string()));
    }
    let Some(layout) = detect_layout(root) else {
        return Err(crate::AdapterError::ProjectNotFound(root.to_string()));
    };
    let base = layout.asset_root();

    // Plain (unencrypted) file.
    if let Ok(bytes) = std::fs::read(base.join(rel).as_std_path()) {
        return Ok(bytes);
    }

    // Encrypted variants: strip `.png`, try `.rpgmvp` (MV) / `.png_` (MZ).
    if let Some(stem) = rel.strip_suffix(".png") {
        for ext in [".rpgmvp", ".png_"] {
            let enc = base.join(format!("{stem}{ext}"));
            if let Ok(bytes) = std::fs::read(enc.as_std_path()) {
                let Some(key) = encryption_key(&layout.data_dir()) else {
                    return Err(crate::AdapterError::Io(std::io::Error::other(
                        "encrypted image but no encryptionKey in System.json",
                    )));
                };
                let out = decrypt_rpgmaker(&bytes, &key);
                if out.is_empty() {
                    return Err(crate::AdapterError::Io(std::io::Error::other(
                        "encrypted image is truncated or in a nonstandard format",
                    )));
                }
                return Ok(out);
            }
        }
    }

    Err(crate::AdapterError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        rel.to_string(),
    )))
}

/// Reads the project's image `encryptionKey` (hex) from `System.json`, if any.
fn encryption_key(data_dir: &Utf8Path) -> Option<Vec<u8>> {
    #[derive(Deserialize)]
    struct Sys {
        #[serde(default, rename = "encryptionKey")]
        encryption_key: String,
    }
    let text = std::fs::read_to_string(data_dir.join("System.json").as_std_path()).ok()?;
    let sys: Sys = serde_json::from_str(&text).ok()?;
    let s = sys.encryption_key.trim();
    // A real key is even-length ASCII hex. The `is_ascii` guard also keeps the
    // byte-offset slicing below on char boundaries — a garbled multibyte key
    // would otherwise panic (the loader must tolerate junk, not crash).
    if s.is_empty() || !s.is_ascii() || !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Decrypts an RPG Maker MV/MZ encrypted asset: a 16-byte fake header is
/// dropped, then the first 16 bytes of the real content are XORed with the key.
fn decrypt_rpgmaker(bytes: &[u8], key: &[u8]) -> Vec<u8> {
    if bytes.len() <= 16 {
        return Vec::new();
    }
    let mut out = bytes[16..].to_vec();
    let n = out.len().min(16).min(key.len());
    for (i, b) in out.iter_mut().take(n).enumerate() {
        *b ^= key[i];
    }
    out
}

#[cfg(test)]
mod graph_tests {
    use super::*;

    /// Root of the synthetic MZ fixture (shared with the CLI integration tests).
    fn fixture_root() -> camino::Utf8PathBuf {
        camino::Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("testdata")
            .join("mz-fixture")
    }

    #[test]
    fn read_project_image_rejects_traversal_and_absolute() {
        let root = fixture_root();
        // Parent-dir traversal and leading separators are rejected regardless of
        // whether the file exists.
        assert!(read_project_image(&root, "../../../etc/passwd").is_err());
        assert!(read_project_image(&root, "img/../../secret.png").is_err());
        assert!(read_project_image(&root, "/etc/passwd").is_err());
        assert!(read_project_image(&root, "\\Windows\\win.ini").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn read_project_image_rejects_windows_drive_paths() {
        let root = fixture_root();
        // A Windows drive-qualified path must not escape the project root: on
        // Windows `base.join("C:\\…")` discards `base` and reads an arbitrary file.
        assert!(read_project_image(&root, "C:\\Windows\\System32\\drivers\\etc\\hosts").is_err());
        assert!(read_project_image(&root, "C:secret.png").is_err());
    }

    #[test]
    fn map_graph_reports_start_edges_and_broken_target() {
        let g = map_graph(&fixture_root()).expect("fixture graph loads");
        assert_eq!(g.start_map_id, 1, "startMapId from System.json");

        let ids: std::collections::BTreeSet<u32> = g.nodes.iter().map(|n| n.id).collect();
        assert!(
            ids.contains(&1) && ids.contains(&2) && ids.contains(&3),
            "maps 1..3 present"
        );
        // The missing transfer target (99) is not materialized as a node.
        assert!(!ids.contains(&99), "nonexistent map 99 is not a node");

        let has = |from: u32, to: u32| g.edges.iter().any(|e| e.from == from && e.to == to);
        assert!(has(1, 2), "direct transfer 1->2");
        assert!(
            has(1, 99),
            "broken transfer 1->99 is still emitted as an edge"
        );

        // The fixture uses only direct transfers.
        assert!(
            g.nodes.iter().all(|n| n.dynamic_exits == 0),
            "no dynamic exits"
        );
    }

    #[test]
    fn map_graph_deduplicates_edges() {
        let g = map_graph(&fixture_root()).expect("fixture graph loads");
        let mut seen = std::collections::BTreeSet::new();
        for e in &g.edges {
            assert!(
                seen.insert((e.from, e.to)),
                "edge {}->{} duplicated",
                e.from,
                e.to
            );
        }
    }
}
