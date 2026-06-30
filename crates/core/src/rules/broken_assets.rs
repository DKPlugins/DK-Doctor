//! Rule `broken-assets`: a reference to a file that is not present on disk.
//!
//! Every site from `asset_refs` whose key is absent from `assets_present`
//! (the set accounts for the adapter's encryption normalization) is a broken
//! asset reference. The image/sound will fail to load.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::{AssetKind, DbKind, Edge, Entity, Ir, Location, PathSeg};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};
use std::collections::HashSet;

/// Rule that finds references to missing assets.
pub struct BrokenAssets;

/// Kinds for which broken-assets is deferred until Layer-A / usage-gating.
///
/// On real projects they produce a flood of false positives: enemies
/// (`Enemy`/`SvEnemy`) are often loaded by a plugin battler system from
/// non-standard folders; animations (`Animation`) — MV ships the full RTP
/// `Animations.json` (~160 entries) but only ships sheets for the ones used;
/// Effekseer `Effect`s — likewise. The existence of the entry is still checked
/// via `referential-integrity` (animationId/enemyId), and the references remain
/// in `asset_refs` (needed for orphan-assets) — here we just don't raise an
/// `error`.
fn is_deferred_kind(kind: AssetKind) -> bool {
    matches!(
        kind,
        AssetKind::Animation | AssetKind::Effect | AssetKind::Enemy | AssetKind::SvEnemy
    )
}

/// Usage-gating of battle backgrounds: a reference to a map's battlebacks is
/// only a "declaration" in `MapXXX.json`. A battleback is loaded only when a
/// battle starts, and a map that cannot start a battle (`!can_battle`: no
/// encounters and no command 301) will never load it — so a missing file is not
/// a bug. Therefore we skip the `error` for battlebacks of a map with
/// `!can_battle`. The reference remains in `asset_refs` (needed for
/// orphan-assets), while System/title/battle-test battlebacks (a path without a
/// `Map` segment) are still checked.
fn battleback_gated_out(ir: &Ir, kind: AssetKind, location: &Location) -> bool {
    if !matches!(kind, AssetKind::Battleback1 | AssetKind::Battleback2) {
        return false;
    }
    let Some(PathSeg::Map(map_id)) = location.path.0.first() else {
        return false;
    };
    let Some(&map_eid) = ir.maps_by_id.get(map_id) else {
        return false;
    };
    matches!(ir.entity(map_eid).map(|n| &n.kind), Some(Entity::Map(m)) if !m.can_battle)
}

/// Ids of tilesets actually used by at least one map (a map's `tilesetId` →
/// `Edge::ReferencesDbId{Tileset}`). Tilesets defined in the database but used by
/// no map are not gameplay-relevant, so their missing images are not flagged.
fn used_tileset_ids(ir: &Ir) -> HashSet<u32> {
    ir.edges
        .iter()
        .filter_map(|r| match r.edge {
            Edge::ReferencesDbId {
                kind: DbKind::Tileset,
                id,
            } => Some(id),
            _ => None,
        })
        .collect()
}

/// Usage-gating for tileset image slots: a tileset asset ref (anchored on its
/// `Tilesets.json` record) is only flagged when some map uses that tileset.
fn tileset_gated_out(kind: AssetKind, location: &Location, used: &HashSet<u32>) -> bool {
    if kind != AssetKind::Tileset {
        return false;
    }
    match location.path.0.first() {
        Some(PathSeg::DbRecord { file, id }) if *file == "Tilesets" => !used.contains(id),
        _ => false,
    }
}

impl Rule for BrokenAssets {
    fn id(&self) -> &'static str {
        "broken-assets"
    }

    fn category(&self) -> Category {
        Category::Asset
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let used_tilesets = used_tileset_ids(ctx.ir);
        let mut findings = Vec::new();
        for (key, location) in &ctx.ir.asset_refs {
            if is_deferred_kind(key.kind) {
                continue;
            }
            if battleback_gated_out(ctx.ir, key.kind, location) {
                continue;
            }
            if tileset_gated_out(key.kind, location, &used_tilesets) {
                continue;
            }
            if ctx.ir.assets_present.contains(key) {
                continue;
            }
            // An asset declared by a plugin (`@type file`): its loading is
            // handled by the plugin (possibly from a non-standard folder) — not broken.
            if ctx.ir.plugin_provided_assets.contains(key) {
                continue;
            }
            findings.push(Finding {
                severity: Severity::Error,
                category: Category::Asset,
                confidence: Confidence::Certain,
                location: location.clone(),
                message: Msg::BrokenAsset {
                    folder: key.kind.folder().to_string(),
                    name: key.name.clone(),
                },
                references: Vec::new(),
                rule: "broken-assets",
            });
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{AssetKey, Engine, Ir, Location, Map, PathSeg};

    fn push_map(b: &mut crate::ir::IrBuilder, map_id: u32, can_battle: bool) {
        b.push_entity(
            Entity::Map(Map {
                map_id,
                name: String::new(),
                event_ids: vec![],
                can_battle,
            }),
            Location::new(
                format!("data/Map{map_id:03}.json"),
                vec![PathSeg::Map(map_id)],
            ),
        );
    }

    fn map_battleback_ref(map_id: u32) -> Location {
        Location::new(
            format!("data/Map{map_id:03}.json"),
            vec![PathSeg::Map(map_id)],
        )
    }

    #[test]
    fn skips_missing_battleback_for_map_that_cannot_battle() {
        let mut b = Ir::builder(Engine::Mz);
        // Map 1 cannot start a battle → its missing battleback is not flagged.
        push_map(&mut b, 1, false);
        b.add_asset_ref(
            AssetKey::new(AssetKind::Battleback1, "Cave"),
            map_battleback_ref(1),
        );
        // Map 2 can start a battle → its missing battleback is flagged.
        push_map(&mut b, 2, true);
        b.add_asset_ref(
            AssetKey::new(AssetKind::Battleback1, "Field"),
            map_battleback_ref(2),
        );
        // System battleback (a path without a Map segment) — always checked.
        b.add_asset_ref(
            AssetKey::new(AssetKind::Battleback2, "Sky"),
            Location::file_only("data/System.json"),
        );
        let ir = b.finish();
        let f = BrokenAssets.run(&RuleCtx::new(&ir));
        let names: Vec<&str> = f
            .iter()
            .filter_map(|x| match &x.message {
                Msg::BrokenAsset { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert!(names.contains(&"Field"));
        assert!(names.contains(&"Sky"));
        assert!(!names.contains(&"Cave"));
        assert_eq!(f.len(), 2);
    }

    #[test]
    fn flags_only_missing_asset() {
        let mut b = Ir::builder(Engine::Mz);
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "Hero"));
        // A reference to a present asset — fine.
        b.add_asset_ref(
            AssetKey::new(AssetKind::Picture, "Hero"),
            Location::file_only("data/Map001.json"),
        );
        // A reference to a missing asset — error.
        b.add_asset_ref(
            AssetKey::new(AssetKind::Picture, "Ghost"),
            Location::file_only("data/Map001.json"),
        );
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = BrokenAssets.run(&ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Error);
        assert!(matches!(
            &f[0].message,
            Msg::BrokenAsset { folder, name }
                if folder == "img/pictures" && name == "Ghost"
        ));
    }

    #[test]
    fn skips_deferred_kinds() {
        let mut b = Ir::builder(Engine::Mz);
        // Missing enemy and animation — deferred, not flagged.
        for kind in [
            AssetKind::Enemy,
            AssetKind::SvEnemy,
            AssetKind::Animation,
            AssetKind::Effect,
        ] {
            b.add_asset_ref(
                AssetKey::new(kind, "Missing"),
                Location::file_only("data/Enemies.json"),
            );
        }
        // Missing picture — flagged (control).
        b.add_asset_ref(
            AssetKey::new(AssetKind::Picture, "Ghost"),
            Location::file_only("data/Map001.json"),
        );
        let ir = b.finish();
        let f = BrokenAssets.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert!(matches!(&f[0].message, Msg::BrokenAsset { name, .. } if name == "Ghost"));
    }

    #[test]
    fn tileset_image_flagged_only_when_tileset_is_used() {
        use crate::ir::{DbKind, Edge};
        let mut b = Ir::builder(Engine::Mz);
        // A map that uses tileset 1.
        let map = b.push_entity(
            Entity::Map(Map {
                map_id: 1,
                name: String::new(),
                event_ids: vec![],
                can_battle: false,
            }),
            Location::new("data/Map001.json", vec![PathSeg::Map(1)]),
        );
        b.push_edge(
            map,
            Edge::ReferencesDbId {
                kind: DbKind::Tileset,
                id: 1,
            },
            Location::file_only("data/Map001.json"),
        );
        // Tileset 1 (used) references a missing image → flagged.
        b.add_asset_ref(
            AssetKey::new(AssetKind::Tileset, "World_A1"),
            Location::new(
                "data/Tilesets.json",
                vec![PathSeg::DbRecord {
                    file: "Tilesets",
                    id: 1,
                }],
            ),
        );
        // Tileset 2 (unused by any map) references a missing image → skipped.
        b.add_asset_ref(
            AssetKey::new(AssetKind::Tileset, "Unused_A1"),
            Location::new(
                "data/Tilesets.json",
                vec![PathSeg::DbRecord {
                    file: "Tilesets",
                    id: 2,
                }],
            ),
        );
        let ir = b.finish();
        let f = BrokenAssets.run(&RuleCtx::new(&ir));
        let names: Vec<&str> = f
            .iter()
            .filter_map(|x| match &x.message {
                Msg::BrokenAsset { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert!(names.contains(&"World_A1"));
        assert!(!names.contains(&"Unused_A1"));
        assert_eq!(f.len(), 1);
    }
}
