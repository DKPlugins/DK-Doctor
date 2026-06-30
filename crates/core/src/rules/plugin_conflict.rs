//! Rule `plugin-conflict`: one core method is patched by ≥2 enabled plugins.
//!
//! The adapter (Tier B, AST heuristic) extracts from the JS of each ENABLED plugin
//! assignments of the form `X.prototype.m = …` and marks each one as:
//!  - **alias-preserving** (`overwrites=false`) — somewhere in the file the original
//!    is saved (`const _m = X.prototype.m`), the new implementation is cooperative;
//!  - **overwriting** (`overwrites=true`) — an assignment without saving the original:
//!    it silently clobbers the method's previous implementation.
//!
//! A conflict is raised when **≥2 distinct enabled plugins** patch the same
//! method AND **at least one overwrites** — then the load order decides, and a
//! later plugin may lose the logic of an earlier one. Pure alias chains (all save
//! the original) are cooperative and are NOT flagged (otherwise a flood on
//! VisuStella/Yanfly, where plugins routinely alias each other). Confidence `likely`.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::{Location, PathSeg};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};
use rustc_hash::{FxHashMap, FxHashSet};

/// Rule for finding plugin conflicts over a shared patched method.
pub struct PluginConflict;

/// Symmetric map of declared relations between plugins from `@base`/`@orderAfter`/
/// `@orderBefore`: `rel[a]` contains everyone with whom `a` has an order declaration
/// (in either direction). Such plugins form a cooperative family (the order is set by
/// the author), and their mutual patches of one method are NOT treated as an unexpected conflict.
fn declared_relations(meta: &crate::ir::PluginMeta) -> FxHashMap<&str, FxHashSet<&str>> {
    let mut rel: FxHashMap<&str, FxHashSet<&str>> = FxHashMap::default();
    for dep in &meta.order_deps {
        let a = dep.plugin.as_str();
        for other in dep
            .base
            .iter()
            .chain(&dep.order_after)
            .chain(&dep.order_before)
        {
            let b = other.as_str();
            rel.entry(a).or_default().insert(b);
            rel.entry(b).or_default().insert(a);
        }
    }
    rel
}

/// Whether all patchers of a method are pairwise linked by a declared dependency
/// (cooperative family → not flagged). An empty/single list is considered "linked".
fn all_pairwise_related(rel: &FxHashMap<&str, FxHashSet<&str>>, patchers: &[(&str, bool)]) -> bool {
    for i in 0..patchers.len() {
        for j in (i + 1)..patchers.len() {
            let (a, b) = (patchers[i].0, patchers[j].0);
            let linked = rel.get(a).is_some_and(|s| s.contains(b));
            if !linked {
                return false;
            }
        }
    }
    true
}

/// The plugin author's "namespace" prefix: the part of the name before the first `_`
/// (≥2 characters). `SRPG_core` → `SRPG`, `NRP_DynamicMotionMap` → `NRP`. Names without
/// `_` → `None` (they do not form a family of their own).
fn author_prefix(name: &str) -> Option<&str> {
    let prefix = name.split_once('_')?.0;
    (prefix.len() >= 2).then_some(prefix)
}

/// Whether all patchers belong to one author/family (a shared name prefix).
/// Add-ons by the same author (`SRPG_core`/`SRPG_AIControl`, `NRP_*`) routinely
/// extend each other's methods — the order is set by the author, not an unexpected conflict.
fn same_author_family(patchers: &[(&str, bool)]) -> bool {
    let Some(first) = patchers.first().and_then(|(p, _)| author_prefix(p)) else {
        return false;
    };
    // Case-insensitive: authors are inconsistent with casing (`NRP_`/`nrp_`),
    // and this only ever SUPPRESSES conflicts within one family.
    patchers
        .iter()
        .all(|(p, _)| author_prefix(p).is_some_and(|x| x.eq_ignore_ascii_case(first)))
}

/// The finding's location — the plugin file (`js/plugins/<name>.js`) + plugin segment.
fn plugin_location(name: &str) -> Location {
    Location::new(
        format!("js/plugins/{name}.js"),
        vec![PathSeg::Plugin(name.to_string())],
    )
}

impl Rule for PluginConflict {
    fn id(&self) -> &'static str {
        "plugin-conflict"
    }

    fn category(&self) -> Category {
        Category::PluginConflict
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let meta = &ctx.ir.plugin_meta;
        // Plugin index in load order (for sorting and the "enabled" filter).
        let order: FxHashMap<&str, usize> = meta
            .load_order
            .iter()
            .enumerate()
            .map(|(i, n)| (n.as_str(), i))
            .collect();

        // method → Vec<(plugin, overwrites)> (one patch per plugin — the adapter
        // already collapsed them; here we additionally dedup just in case).
        let mut by_method: FxHashMap<&str, Vec<(&str, bool)>> = FxHashMap::default();
        for patch in &meta.patches {
            // Only enabled plugins (patches of disabled ones are not executed).
            if !order.contains_key(patch.plugin.as_str()) {
                continue;
            }
            let entry = by_method.entry(patch.method.as_str()).or_default();
            if let Some(existing) = entry.iter_mut().find(|(p, _)| *p == patch.plugin) {
                existing.1 |= patch.overwrites;
            } else {
                entry.push((patch.plugin.as_str(), patch.overwrites));
            }
        }

        let rel = declared_relations(meta);
        let mut findings = Vec::new();
        // Deterministic output order — by method name.
        let mut methods: Vec<&str> = by_method.keys().copied().collect();
        methods.sort_unstable();

        for method in methods {
            let mut patchers = by_method.remove(method).unwrap();
            // ≥2 distinct plugins and at least one overwrites.
            if patchers.len() < 2 || !patchers.iter().any(|(_, ow)| *ow) {
                continue;
            }
            // Cooperative family — the order is set by the author, not an unexpected
            // conflict: either all are pairwise linked via @base/@orderAfter/@orderBefore,
            // or all are by the same author (shared name prefix, `SRPG_*`/`NRP_*`).
            if all_pairwise_related(&rel, &patchers) || same_author_family(&patchers) {
                continue;
            }
            // Sort by load order.
            patchers.sort_by_key(|(p, _)| order.get(*p).copied().unwrap_or(usize::MAX));
            let plugins: Vec<String> = patchers.iter().map(|(p, _)| p.to_string()).collect();
            let overwriters: Vec<String> = patchers
                .iter()
                .filter(|(_, ow)| *ow)
                .map(|(p, _)| p.to_string())
                .collect();

            // The primary location — the last overwriter in order (it "wins").
            let primary = overwriters
                .last()
                .cloned()
                .unwrap_or_else(|| plugins[0].clone());
            let references = plugins
                .iter()
                .filter(|p| **p != primary)
                .map(|p| plugin_location(p))
                .collect();

            findings.push(Finding {
                severity: Severity::Warning,
                category: Category::PluginConflict,
                confidence: Confidence::Likely,
                location: plugin_location(&primary),
                message: Msg::PluginConflict {
                    method: method.to_string(),
                    plugins,
                    overwriters,
                },
                references,
                rule: "plugin-conflict",
            });
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Engine, Ir, MethodPatch, PluginMeta};

    fn ir_with(load_order: &[&str], patches: &[(&str, &str, bool)]) -> Ir {
        ir_full(load_order, patches, &[])
    }

    /// Variant with declared dependencies: `deps` = `(plugin, depends_on)`.
    fn ir_full(load_order: &[&str], patches: &[(&str, &str, bool)], deps: &[(&str, &str)]) -> Ir {
        use crate::ir::PluginOrderDeps;
        let mut b = Ir::builder(Engine::Mz);
        let mut meta = PluginMeta::new();
        meta.load_order = load_order.iter().map(|s| s.to_string()).collect();
        meta.patches = patches
            .iter()
            .map(|(plugin, method, overwrites)| MethodPatch {
                method: method.to_string(),
                plugin: plugin.to_string(),
                overwrites: *overwrites,
            })
            .collect();
        for (plugin, dep) in deps {
            meta.order_deps.push(PluginOrderDeps {
                plugin: plugin.to_string(),
                base: vec![dep.to_string()],
                order_after: vec![],
                order_before: vec![],
            });
        }
        b.set_plugin_meta(meta);
        b.finish()
    }

    #[test]
    fn flags_two_overwriters_of_same_method() {
        let ir = ir_with(
            &["A", "B"],
            &[
                ("A", "Game_Battler.prototype.gainHp", true),
                ("B", "Game_Battler.prototype.gainHp", true),
            ],
        );
        let f = PluginConflict.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "plugin-conflict");
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(f[0].confidence, Confidence::Likely);
        assert!(matches!(
            &f[0].message,
            Msg::PluginConflict { method, plugins, overwriters }
                if method == "Game_Battler.prototype.gainHp"
                    && plugins == &["A".to_string(), "B".to_string()]
                    && overwriters.len() == 2
        ));
    }

    #[test]
    fn spares_pure_alias_chain() {
        // Both plugins alias (save the original) — cooperative, not a conflict.
        let ir = ir_with(
            &["A", "B"],
            &[
                ("A", "Scene_Map.prototype.update", false),
                ("B", "Scene_Map.prototype.update", false),
            ],
        );
        assert!(PluginConflict.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn flags_when_at_least_one_overwrites() {
        // A aliases, B overwrites → B silently clobbers A (if loaded after).
        let ir = ir_with(
            &["A", "B"],
            &[
                ("A", "Window_Base.prototype.drawText", false),
                ("B", "Window_Base.prototype.drawText", true),
            ],
        );
        let f = PluginConflict.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert!(matches!(
            &f[0].message,
            Msg::PluginConflict { overwriters, .. } if overwriters == &["B".to_string()]
        ));
    }

    #[test]
    fn spares_single_patcher() {
        let ir = ir_with(&["A"], &[("A", "Game_Map.prototype.setup", true)]);
        assert!(PluginConflict.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn spares_declared_family_even_when_overwriting() {
        // B declares @base A → cooperative family, the order is set by the author.
        // Even when overwriting one method — not flagged (anti-flood of NRP families).
        let ir = ir_full(
            &["A", "B"],
            &[
                ("A", "Game_Battler.prototype.gainHp", true),
                ("B", "Game_Battler.prototype.gainHp", true),
            ],
            &[("B", "A")],
        );
        assert!(PluginConflict.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn flags_unrelated_pair_within_larger_set() {
        // A↔B linked (@base), but C is linked to no one → a real conflict A/C/B.
        let ir = ir_full(
            &["A", "B", "C"],
            &[
                ("A", "Scene_Battle.prototype.update", true),
                ("B", "Scene_Battle.prototype.update", true),
                ("C", "Scene_Battle.prototype.update", true),
            ],
            &[("B", "A")],
        );
        assert_eq!(PluginConflict.run(&RuleCtx::new(&ir)).len(), 1);
    }

    #[test]
    fn spares_same_author_family_by_prefix() {
        // SRPG_core and SRPG_AIControl — one author (prefix SRPG_), routinely
        // extend one method. Not flagged (anti-flood of SRPG/NRP families).
        let ir = ir_with(
            &["SRPG_core", "SRPG_AIControl"],
            &[
                ("SRPG_core", "Scene_Map.prototype.update", true),
                ("SRPG_AIControl", "Scene_Map.prototype.update", true),
            ],
        );
        assert!(PluginConflict.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn spares_same_author_family_case_insensitive() {
        // Same author, inconsistent casing of the prefix → still one family.
        let ir = ir_with(
            &["NRP_Foo", "nrp_Bar"],
            &[
                ("NRP_Foo", "Game_Map.prototype.update", true),
                ("nrp_Bar", "Game_Map.prototype.update", true),
            ],
        );
        assert!(PluginConflict.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn flags_cross_author_overwrite() {
        // Different authors (NRP_ vs ABS_) overwrite one core method → conflict.
        let ir = ir_with(
            &["NRP_Foo", "ABS_Bar"],
            &[
                ("NRP_Foo", "Game_Battler.prototype.gainHp", true),
                ("ABS_Bar", "Game_Battler.prototype.gainHp", true),
            ],
        );
        assert_eq!(PluginConflict.run(&RuleCtx::new(&ir)).len(), 1);
    }

    #[test]
    fn ignores_disabled_plugin_patches() {
        // B is not in load_order (disabled) → not counted, no conflict.
        let ir = ir_with(
            &["A"],
            &[
                ("A", "Game_Battler.prototype.gainHp", true),
                ("B", "Game_Battler.prototype.gainHp", true),
            ],
        );
        assert!(PluginConflict.run(&RuleCtx::new(&ir)).is_empty());
    }
}
