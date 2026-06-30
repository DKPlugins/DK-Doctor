//! Rule `plugin-load-order`: the plugin's declared order is violated by the
//! actual order in `plugins.js`.
//!
//! A plugin may declare `@base <X>` / `@orderAfter <X>` (X must load EARLIER)
//! or `@orderBefore <X>` (X must load LATER). The actual order is the sequence
//! of enabled plugins in `plugins.js`
//! ([`crate::ir::PluginMeta::load_order`]). If a dependency ends up on the wrong
//! side, the order is violated: the plugin initializes before its base and may
//! crash or behave incorrectly.
//!
//! Tier A, confidence `Certain`: both the requirement and the order are explicit
//! data, no heuristics involved. A missing/disabled `@base` plugin is a separate
//! `missing-base` rule (here we only check the relative order of present ones).

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::Location;
use crate::message::{Msg, PluginOrderTag};
use crate::rules::{Rule, RuleCtx};
use rustc_hash::FxHashMap;

/// Rule that checks the plugin load order against their declarations.
pub struct PluginLoadOrder;

impl Rule for PluginLoadOrder {
    fn id(&self) -> &'static str {
        "plugin-load-order"
    }

    fn category(&self) -> Category {
        Category::PluginOrder
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let meta = &ctx.ir.plugin_meta;
        // Position of each enabled plugin in the load order.
        let pos: FxHashMap<&str, usize> = meta
            .load_order
            .iter()
            .enumerate()
            .map(|(i, name)| (name.as_str(), i))
            .collect();

        let mut findings = Vec::new();
        for deps in &meta.order_deps {
            let Some(&self_pos) = pos.get(deps.plugin.as_str()) else {
                continue;
            };
            // @base and @orderAfter: the dependency must come EARLIER (smaller index).
            for (dep, tag) in deps.base.iter().map(|d| (d, PluginOrderTag::Base)).chain(
                deps.order_after
                    .iter()
                    .map(|d| (d, PluginOrderTag::OrderAfter)),
            ) {
                if let Some(&dep_pos) = pos.get(dep.as_str())
                    && dep_pos > self_pos
                {
                    findings.push(violation(&deps.plugin, dep, tag));
                }
            }
            // @orderBefore: the dependency must come LATER (larger index).
            for dep in &deps.order_before {
                if let Some(&dep_pos) = pos.get(dep.as_str())
                    && dep_pos < self_pos
                {
                    findings.push(violation(&deps.plugin, dep, PluginOrderTag::OrderBefore));
                }
            }
        }
        findings
    }
}

/// Constructs an order-violation finding (location is `js/plugins.js`).
fn violation(plugin: &str, dependency: &str, tag: PluginOrderTag) -> Finding {
    Finding {
        severity: Severity::Error,
        category: Category::PluginOrder,
        confidence: Confidence::Certain,
        location: Location::file_only("js/plugins.js"),
        message: Msg::PluginLoadOrder {
            plugin: plugin.to_string(),
            dependency: dependency.to_string(),
            tag,
        },
        references: Vec::new(),
        rule: "plugin-load-order",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Engine, Ir, PluginMeta, PluginOrderDeps};

    fn ir_with(load_order: &[&str], deps: Vec<PluginOrderDeps>) -> Ir {
        let mut b = Ir::builder(Engine::Mz);
        let mut meta = PluginMeta::new();
        meta.load_order = load_order.iter().map(|s| s.to_string()).collect();
        meta.order_deps = deps;
        b.set_plugin_meta(meta);
        b.finish()
    }

    fn order_after(plugin: &str, after: &str) -> PluginOrderDeps {
        PluginOrderDeps {
            plugin: plugin.to_string(),
            base: vec![],
            order_after: vec![after.to_string()],
            order_before: vec![],
        }
    }

    #[test]
    fn flags_base_loaded_after_dependent() {
        // Plugin depends (@base) on Core, but Core comes AFTER Plugin → violation.
        let deps = vec![PluginOrderDeps {
            plugin: "Plugin".to_string(),
            base: vec!["Core".to_string()],
            order_after: vec![],
            order_before: vec![],
        }];
        let ir = ir_with(&["Plugin", "Core"], deps);
        let f = PluginLoadOrder.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Error);
        assert_eq!(f[0].confidence, Confidence::Certain);
        assert!(matches!(
            &f[0].message,
            Msg::PluginLoadOrder { plugin, dependency, tag: PluginOrderTag::Base }
                if plugin == "Plugin" && dependency == "Core"
        ));
    }

    #[test]
    fn accepts_correct_order_after() {
        // Core before Plugin → @orderAfter satisfied.
        let ir = ir_with(&["Core", "Plugin"], vec![order_after("Plugin", "Core")]);
        assert!(PluginLoadOrder.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn flags_order_before_violation() {
        // Plugin must load BEFORE Late, but Late comes earlier → violation.
        let deps = vec![PluginOrderDeps {
            plugin: "Plugin".to_string(),
            base: vec![],
            order_after: vec![],
            order_before: vec!["Late".to_string()],
        }];
        let ir = ir_with(&["Late", "Plugin"], deps);
        let f = PluginLoadOrder.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert!(matches!(
            &f[0].message,
            Msg::PluginLoadOrder {
                tag: PluginOrderTag::OrderBefore,
                ..
            }
        ));
    }

    #[test]
    fn ignores_absent_dependency() {
        // The dependency is absent from the load order → that's missing-base's
        // concern, the relative order here is not violated.
        let ir = ir_with(&["Plugin"], vec![order_after("Plugin", "Ghost")]);
        assert!(PluginLoadOrder.run(&RuleCtx::new(&ir)).is_empty());
    }
}
