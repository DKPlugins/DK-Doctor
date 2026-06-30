//! Rule `dead-variables`: a variable that is written to but never read.
//!
//! A pure query to [`SymbolTable`]: a variable with ≥1 write site and **0**
//! read sites — it is written, but never used anywhere (dead state).

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};

/// Rule that finds dead variables (written, but never read).
pub struct DeadVariables;

impl Rule for DeadVariables {
    fn id(&self) -> &'static str {
        "dead-variables"
    }

    fn category(&self) -> Category {
        Category::Data
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        for info in ctx.ir.symbols.variables.values() {
            if info.writes.is_empty() || !info.reads.is_empty() {
                continue;
            }
            // Read from plugin/script JS ($gameVariables.value(N)) — the write is
            // not dead, the value is consumed at runtime. Suppress (keeps the
            // `certain` confidence honest: a flagged write is a real dead write).
            if info.read_by_plugin {
                continue;
            }
            let name = info
                .name
                .as_deref()
                .filter(|n| !n.is_empty())
                .map(String::from);
            let location = info.writes[0].location.clone();
            let references = info.writes.iter().map(|s| s.location.clone()).collect();
            findings.push(Finding {
                severity: Severity::Warning,
                category: Category::Data,
                confidence: Confidence::Certain,
                location,
                message: Msg::DeadVariable {
                    id: info.id,
                    name,
                    writes: info.writes.len(),
                },
                references,
                rule: "dead-variables",
            });
        }
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
    fn flags_written_never_read_and_spares_read_variable() {
        let mut b = Ir::builder(Engine::Mz);
        // #7: written, not read → dead.
        b.symbols_mut()
            .add_variable_write(7, site("data/Map001.json"));
        b.symbols_mut()
            .add_variable_write(7, site("data/Map002.json"));
        // #9: written AND read → not dead (control).
        b.symbols_mut()
            .add_variable_write(9, site("data/Map001.json"));
        b.symbols_mut()
            .add_variable_read(9, site("data/Map003.json"));
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = DeadVariables.run(&ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "dead-variables");
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(f[0].confidence, Confidence::Certain);
        assert_eq!(f[0].references.len(), 2);
        assert!(matches!(
            f[0].message,
            Msg::DeadVariable {
                id: 7,
                writes: 2,
                ..
            }
        ));
    }

    #[test]
    fn spares_variable_read_only_from_plugin_js() {
        let mut b = Ir::builder(Engine::Mz);
        // #5: written by data, but read only via $gameVariables.value(5) in JS →
        // not dead, must be suppressed.
        b.symbols_mut()
            .add_variable_write(5, site("data/Map001.json"));
        b.symbols_mut().mark_variable_read_by_plugin(5);
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);
        assert!(
            DeadVariables.run(&ctx).is_empty(),
            "variable read from JS is not a dead write"
        );
    }
}
