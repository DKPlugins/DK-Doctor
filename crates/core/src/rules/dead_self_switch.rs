//! Rule `dead-self-switch`: a self-switch that is set but never checked.
//!
//! A pure query over [`SelfSwitchTable`](crate::ir::SelfSwitchTable): a self-switch
//! `(map, event, ch)` with ≥1 write site (command 123) and **0** read sites
//! (page condition or command 111 type 2) — it is set but never
//! checked (a dead self-switch: the write has no effect).

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};

/// Rule for finding dead self-switches (written but never read).
pub struct DeadSelfSwitch;

impl Rule for DeadSelfSwitch {
    fn id(&self) -> &'static str {
        "dead-self-switch"
    }

    fn category(&self) -> Category {
        Category::Data
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        for (key, info) in &ctx.ir.self_switches.entries {
            if info.writes.is_empty() || !info.reads.is_empty() {
                continue;
            }
            let location = info.writes[0].location.clone();
            let references = info.writes.iter().map(|s| s.location.clone()).collect();
            findings.push(Finding {
                severity: Severity::Warning,
                category: Category::Data,
                confidence: Confidence::Certain,
                location,
                message: Msg::DeadSelfSwitch {
                    ch: key.ch,
                    event: key.event_id,
                },
                references,
                rule: "dead-self-switch",
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
    fn flags_written_never_read_and_spares_read_self_switch() {
        let mut b = Ir::builder(Engine::Mz);
        // (1, 5, 'A'): written, never read → dead.
        b.add_self_switch_write(SelfSwitchKey::new(1, 5, 'A'), site("data/Map001.json"));
        // (1, 6, 'B'): written AND read → fine (control).
        b.add_self_switch_write(SelfSwitchKey::new(1, 6, 'B'), site("data/Map001.json"));
        b.add_self_switch_read(SelfSwitchKey::new(1, 6, 'B'), site("data/Map001.json"));
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = DeadSelfSwitch.run(&ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "dead-self-switch");
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(f[0].confidence, Confidence::Certain);
        assert!(matches!(
            f[0].message,
            Msg::DeadSelfSwitch { ch: 'A', event: 5 }
        ));
    }
}
