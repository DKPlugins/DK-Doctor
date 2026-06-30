//! Rule `unreachable-self-switch`: a page requires a self-switch that
//! nobody sets.
//!
//! A self-switch `(map, event, ch)` with ≥1 read site (page condition or
//! command 111 type 2) and **0** write sites (command 123) — the condition will
//! never become true, the page is unreachable. Confidence is `likely` with a
//! disclaimer: self-switches may be set by plugins/scripts
//! (`$gameSelfSwitches`), and the plugin layer is not analyzed in this iteration
//! (mirrors the tone of `uninitialized-symbols`).

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};

/// Rule for finding unreachable pages caused by a self-switch that is never set.
pub struct UnreachableSelfSwitch;

impl Rule for UnreachableSelfSwitch {
    fn id(&self) -> &'static str {
        "unreachable-self-switch"
    }

    fn category(&self) -> Category {
        Category::Reference
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        for (key, info) in &ctx.ir.self_switches.entries {
            if info.reads.is_empty() || !info.writes.is_empty() {
                continue;
            }
            let location = info.reads[0].location.clone();
            let references = info.reads.iter().map(|s| s.location.clone()).collect();
            findings.push(Finding {
                severity: Severity::Warning,
                category: Category::Reference,
                confidence: Confidence::Likely,
                location,
                message: Msg::UnreachableSelfSwitch {
                    ch: key.ch,
                    event: key.event_id,
                },
                references,
                rule: "unreachable-self-switch",
            });
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Engine, EntityId, Ir, Location, SelfSwitchKey, Site};

    fn site(file: &str) -> Site {
        Site {
            location: Location::file_only(file),
            entity: EntityId(0),
        }
    }

    #[test]
    fn flags_read_never_written_and_spares_written_self_switch() {
        let mut b = Ir::builder(Engine::Mz);
        // (1, 5, 'C'): read (condition), never written → page is unreachable.
        b.add_self_switch_read(SelfSwitchKey::new(1, 5, 'C'), site("data/Map001.json"));
        // (1, 7, 'D'): read AND written → fine (control case).
        b.add_self_switch_read(SelfSwitchKey::new(1, 7, 'D'), site("data/Map001.json"));
        b.add_self_switch_write(SelfSwitchKey::new(1, 7, 'D'), site("data/Map001.json"));
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = UnreachableSelfSwitch.run(&ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "unreachable-self-switch");
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(f[0].confidence, Confidence::Likely);
        assert!(matches!(
            f[0].message,
            Msg::UnreachableSelfSwitch { ch: 'C', event: 5 }
        ));
    }
}
