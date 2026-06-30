//! Rule `missing-base`: `@base <X>` where X is absent from `plugins.js` or
//! disabled.
//!
//! `@base` is a HARD dependency: the base plugin must be present and
//! enabled, otherwise the dependent plugin will not get the required code and
//! will almost certainly fail at load time (`ReferenceError`). We check every `@base`
//! of every enabled plugin against the set of enabled
//! ([`crate::ir::PluginMeta::load_order`]) and disabled
//! ([`crate::ir::PluginMeta::disabled`]) plugins.
//!
//! Tier A, confidence `Certain`: the fact of absence/disablement is exact data.
//! The mutual ORDER of present bases is checked by `plugin-load-order`.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::Location;
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};
use rustc_hash::FxHashSet;

/// Rule that checks the presence and enablement of `@base` plugins.
pub struct MissingBase;

impl Rule for MissingBase {
    fn id(&self) -> &'static str {
        "missing-base"
    }

    fn category(&self) -> Category {
        Category::PluginOrder
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let meta = &ctx.ir.plugin_meta;
        let enabled: FxHashSet<&str> = meta.load_order.iter().map(String::as_str).collect();
        let disabled: FxHashSet<&str> = meta.disabled.iter().map(String::as_str).collect();

        let mut findings = Vec::new();
        for deps in &meta.order_deps {
            // Order declarations are collected only for enabled plugins.
            for base in &deps.base {
                if enabled.contains(base.as_str()) {
                    continue;
                }
                let is_disabled = disabled.contains(base.as_str());
                findings.push(Finding {
                    severity: Severity::Error,
                    category: Category::PluginOrder,
                    confidence: Confidence::Certain,
                    location: Location::file_only("js/plugins.js"),
                    message: Msg::MissingBase {
                        plugin: deps.plugin.clone(),
                        base: base.clone(),
                        disabled: is_disabled,
                    },
                    references: Vec::new(),
                    rule: "missing-base",
                });
            }
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Engine, Ir, PluginMeta, PluginOrderDeps};

    fn base_dep(plugin: &str, base: &str) -> PluginOrderDeps {
        PluginOrderDeps {
            plugin: plugin.to_string(),
            base: vec![base.to_string()],
            order_after: vec![],
            order_before: vec![],
        }
    }

    fn ir_with(load_order: &[&str], disabled: &[&str], deps: Vec<PluginOrderDeps>) -> Ir {
        let mut b = Ir::builder(Engine::Mz);
        let mut meta = PluginMeta::new();
        meta.load_order = load_order.iter().map(|s| s.to_string()).collect();
        meta.disabled = disabled.iter().map(|s| s.to_string()).collect();
        meta.order_deps = deps;
        b.set_plugin_meta(meta);
        b.finish()
    }

    #[test]
    fn flags_absent_base() {
        let ir = ir_with(&["Plugin"], &[], vec![base_dep("Plugin", "Core")]);
        let f = MissingBase.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Error);
        assert!(matches!(
            &f[0].message,
            Msg::MissingBase { plugin, base, disabled: false }
                if plugin == "Plugin" && base == "Core"
        ));
    }

    #[test]
    fn flags_disabled_base_distinctly() {
        let ir = ir_with(&["Plugin"], &["Core"], vec![base_dep("Plugin", "Core")]);
        let f = MissingBase.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert!(matches!(
            &f[0].message,
            Msg::MissingBase { disabled: true, .. }
        ));
    }

    #[test]
    fn accepts_present_enabled_base() {
        let ir = ir_with(&["Core", "Plugin"], &[], vec![base_dep("Plugin", "Core")]);
        assert!(MissingBase.run(&RuleCtx::new(&ir)).is_empty());
    }
}
