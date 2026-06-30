//! Rule `uninitialized-symbols`: a switch/var that is read but never written.
//!
//! A symbol with ≥1 read site and **0** write sites: the condition will never
//! become true / the value is never set. If the symbol is declared by a plugin
//! (`@type switch`/`variable`, `declared_by_plugin`), it is skipped: the plugin
//! initializes the value at runtime.
//!
//! Confidence depends on whether the plugin layer was parsed (Tier A,
//! `ctx.plugin_decls_available`): if so, the remaining (non-plugin) findings get
//! `Certain` (we CHECKED against `@param` and the symbol is not declared by any
//! enabled plugin); if plugins were not parsed, `Likely` with a disclaimer that
//! the value may have been set by a plugin.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::symbols::SymbolInfo;
use crate::message::{Msg, SymbolKind};
use crate::rules::{Rule, RuleCtx};

/// Rule for finding uninitialized symbols (read but never written).
pub struct UninitSymbols;

impl UninitSymbols {
    fn check<'a>(
        kind: SymbolKind,
        symbols: impl Iterator<Item = &'a SymbolInfo>,
        plugin_checked: bool,
        out: &mut Vec<Finding>,
    ) {
        for info in symbols {
            // Declared (@type, Tier A) OR written by plugin JS code (Tier B)
            // ⇒ the symbol is managed by a plugin at runtime, not "uninitialized".
            if info.reads.is_empty()
                || !info.writes.is_empty()
                || info.declared_by_plugin
                || info.set_by_plugin
            {
                continue;
            }
            let name = info
                .name
                .as_deref()
                .filter(|n| !n.is_empty())
                .map(String::from);
            let location = info.reads[0].location.clone();
            let references = info.reads.iter().map(|s| s.location.clone()).collect();
            // Cross-check against plugins done ⇒ remainder is reliable (`Certain`).
            let confidence = if plugin_checked {
                Confidence::Certain
            } else {
                Confidence::Likely
            };
            out.push(Finding {
                severity: Severity::Warning,
                category: Category::Data,
                confidence,
                location,
                message: Msg::UninitializedSymbol {
                    kind,
                    id: info.id,
                    name,
                    reads: info.reads.len(),
                    plugin_checked,
                },
                references,
                rule: "uninitialized-symbols",
            });
        }
    }
}

impl Rule for UninitSymbols {
    fn id(&self) -> &'static str {
        "uninitialized-symbols"
    }

    fn category(&self) -> Category {
        Category::Data
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        let plugin_checked = ctx.plugin_decls_available;
        Self::check(
            SymbolKind::Switch,
            ctx.ir.symbols.switches.values(),
            plugin_checked,
            &mut findings,
        );
        Self::check(
            SymbolKind::Variable,
            ctx.ir.symbols.variables.values(),
            plugin_checked,
            &mut findings,
        );
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Engine, EntityId, Ir, Location, Site};

    fn site(file: &str) -> Site {
        Site {
            location: Location::file_only(file),
            entity: EntityId(0),
        }
    }

    #[test]
    fn flags_read_never_written_and_spares_written_symbol() {
        let mut b = Ir::builder(Engine::Mz);
        // switch #3: read, not written → uninitialized.
        b.symbols_mut().add_switch_read(3, site("data/Map001.json"));
        // switch #5: read AND written → normal (control).
        b.symbols_mut().add_switch_read(5, site("data/Map001.json"));
        b.symbols_mut()
            .add_switch_write(5, site("data/Map002.json"));
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = UninitSymbols.run(&ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "uninitialized-symbols");
        assert_eq!(f[0].confidence, Confidence::Likely);
        assert_eq!(f[0].severity, Severity::Warning);
        assert!(matches!(
            f[0].message,
            Msg::UninitializedSymbol {
                kind: SymbolKind::Switch,
                id: 3,
                ..
            }
        ));
    }

    #[test]
    fn suppressed_when_declared_by_plugin() {
        let mut b = Ir::builder(Engine::Mz);
        b.symbols_mut()
            .add_variable_read(2, site("data/Map001.json"));
        b.symbols_mut().mark_variable_declared_by_plugin(2);
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);
        assert!(UninitSymbols.run(&ctx).is_empty());
    }

    #[test]
    fn suppressed_when_set_by_plugin_js() {
        // switch #4 is read in the data, but a plugin writes it via JS code (Tier B) ⇒
        // not "uninitialized".
        let mut b = Ir::builder(Engine::Mz);
        b.symbols_mut().add_switch_read(4, site("data/Map001.json"));
        b.symbols_mut().mark_switch_set_by_plugin(4);
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);
        assert!(UninitSymbols.run(&ctx).is_empty());
    }

    #[test]
    fn promotes_to_certain_when_plugins_parsed() {
        // Plugins parsed (load_order non-empty) ⇒ remainder already checked against @param:
        // confidence Certain, plugin_checked=true.
        let mut b = Ir::builder(Engine::Mz);
        b.symbols_mut().add_switch_read(3, site("data/Map001.json"));
        let mut meta = dk_doctor_core_plugin_meta();
        meta.load_order.push("SomePlugin".to_string());
        b.set_plugin_meta(meta);
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);
        assert!(ctx.plugin_decls_available);

        let f = UninitSymbols.run(&ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].confidence, Confidence::Certain);
        assert!(matches!(
            f[0].message,
            Msg::UninitializedSymbol {
                id: 3,
                plugin_checked: true,
                ..
            }
        ));
    }

    /// Empty [`PluginMeta`] for the test (the core does not re-export the constructor here).
    fn dk_doctor_core_plugin_meta() -> crate::ir::PluginMeta {
        crate::ir::PluginMeta::new()
    }
}
