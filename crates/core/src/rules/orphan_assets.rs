//! `orphan-assets` rule: a file on disk that nothing references.
//!
//! Reverse set difference: a key from `assets_present` that is absent among
//! `asset_refs` is an "orphan" candidate. Guards per spec §4.5:
//! - `img/system/` is not scanned by the adapter at all → the engine's set won't end up here;
//! - the MV pair `.ogg`+`.m4a` is already collapsed into a single key during normalization;
//! - leading `$`/`!` are genuine name characters and are preserved;
//! - the `effects/` folder is excluded: non-`.efkefc` files are pulled in transitively via
//!   `.efkefc`, and extensions are unavailable after normalization — we lazily skip everything.
//!
//! Plugins are not parsed in this iteration, so assets referenced only by
//! plugins are still unknown — findings are phrased as "possibly not
//! used" rather than "garbage".
//!
//! Assets managed by a plugin (`plugin_provided_assets`) are excluded from
//! orphans — symmetrically to `broken-assets`: their runtime loading is handled by the plugin
//! from its (possibly non-standard) folder by a name statically invisible
//! from here, so we conservatively consider them used.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::AssetKind;
use crate::ir::asset::AssetKey;
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};
use rustc_hash::FxHashSet;

/// Rule that finds (possibly) unused assets on disk.
pub struct OrphanAssets;

impl Rule for OrphanAssets {
    fn id(&self) -> &'static str {
        "orphan-assets"
    }

    fn category(&self) -> Category {
        Category::Asset
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let referenced: FxHashSet<&AssetKey> = ctx.ir.asset_refs.iter().map(|(k, _)| k).collect();

        let mut orphans: Vec<&AssetKey> = ctx
            .ir
            .assets_present
            .iter()
            .filter(|key| {
                // effects/ — transitive dependencies, not touched in iter1;
                // plugin_provided_assets — managed by a plugin, not orphans.
                key.kind != AssetKind::Effect
                    && !referenced.contains(*key)
                    && !ctx.ir.plugin_provided_assets.contains(*key)
            })
            .collect();
        // Deterministic order for stable snapshots.
        orphans.sort_by(|a, b| {
            a.kind
                .folder()
                .cmp(b.kind.folder())
                .then_with(|| a.name.cmp(&b.name))
        });

        orphans
            .into_iter()
            .map(|key| {
                let folder = key.kind.folder();
                Finding {
                    severity: Severity::Info,
                    category: Category::Asset,
                    confidence: Confidence::Certain,
                    location: crate::ir::Location::file_only(format!("{folder}/{}", key.name)),
                    message: Msg::OrphanAsset {
                        folder: folder.to_string(),
                        name: key.name.clone(),
                    },
                    references: Vec::new(),
                    rule: "orphan-assets",
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Engine, Ir, Location};

    #[test]
    fn flags_unreferenced_present_file_only() {
        let mut b = Ir::builder(Engine::Mz);
        // Hero — present and used.
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "Hero"));
        b.add_asset_ref(
            AssetKey::new(AssetKind::Picture, "Hero"),
            Location::file_only("data/Map001.json"),
        );
        // Unused — present, no references → orphan.
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "Unused"));
        // $Big — leading $ preserved, no references → orphan.
        b.add_asset_present(AssetKey::new(AssetKind::Character, "$Big"));
        // effect without references — NOT an orphan (transitive guard).
        b.add_asset_present(AssetKey::new(AssetKind::Effect, "Explosion"));
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = OrphanAssets.run(&ctx);
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|x| x.severity == Severity::Info));
        assert!(
            f.iter()
                .any(|x| matches!(&x.message, Msg::OrphanAsset { name, .. } if name == "Unused"))
        );
        assert!(
            f.iter()
                .any(|x| matches!(&x.message, Msg::OrphanAsset { name, .. } if name == "$Big"))
        );
        assert!(
            f.iter().all(
                |x| !matches!(&x.message, Msg::OrphanAsset { name, .. } if name == "Explosion")
            )
        );
        assert!(f.iter().all(
            |x| matches!(&x.message, Msg::OrphanAsset { folder, .. } if folder.starts_with("img/"))
        ));
    }

    #[test]
    fn skips_plugin_provided_present_asset() {
        let mut b = Ir::builder(Engine::Mz);
        // busts/X — present, no references, but managed by a plugin → NOT an orphan.
        let bust = AssetKey::new(AssetKind::Picture, "busts/X");
        b.add_asset_present(bust.clone());
        b.add_plugin_provided_asset(bust);
        // Loose — present, no references, not managed by a plugin → orphan (control).
        b.add_asset_present(AssetKey::new(AssetKind::Picture, "Loose"));
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = OrphanAssets.run(&ctx);
        assert_eq!(f.len(), 1);
        assert!(
            f.iter()
                .any(|x| matches!(&x.message, Msg::OrphanAsset { name, .. } if name == "Loose"))
        );
        assert!(
            f.iter()
                .all(|x| !matches!(&x.message, Msg::OrphanAsset { name, .. } if name == "busts/X"))
        );
    }
}
