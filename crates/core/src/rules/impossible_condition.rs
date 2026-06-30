//! Rule `impossible-condition`: a constantly-resolvable condition (dead branch).
//!
//! The lightweight constant-propagation adapter (literals of command 122 within a
//! single command list) has already determined that the condition of command 111
//! is always true or always false, and placed the verdict in
//! [`Ir::dead_branches`](crate::ir::Ir). Here it is turned into a finding: one of
//! the branches (`then`/`else`) is unreachable.
//!
//! Confidence is `likely`: value propagation respects dominance within the list
//! (an assignment higher up at the same/lesser indent, with no overwrite and no
//! opaque commands in between), but the value could have changed outside of static
//! analysis (parallel process, plugin), so we do not promise 100%.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};

/// Rule that finds constant-resolvable conditions (dead branches).
pub struct ImpossibleCondition;

impl Rule for ImpossibleCondition {
    fn id(&self) -> &'static str {
        "impossible-condition"
    }

    fn category(&self) -> Category {
        Category::DeadCode
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        ctx.ir
            .dead_branches
            .iter()
            .map(|db| Finding {
                severity: Severity::Warning,
                category: Category::DeadCode,
                confidence: Confidence::Likely,
                location: db.location.clone(),
                message: Msg::ImpossibleCondition {
                    var_id: db.var_id,
                    value: db.value,
                    op: db.op,
                    operand: db.operand,
                    result: db.result,
                },
                references: Vec::new(),
                rule: "impossible-condition",
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{CmpOp, DeadBranch, Engine, Ir, Location};

    #[test]
    fn emits_finding_per_dead_branch() {
        let mut b = Ir::builder(Engine::Mz);
        b.add_dead_branch(DeadBranch {
            location: Location::file_only("data/Map001.json"),
            var_id: 5,
            value: 0,
            op: CmpOp::Ge,
            operand: 1,
            result: false,
        });
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = ImpossibleCondition.run(&ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "impossible-condition");
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(f[0].confidence, Confidence::Likely);
        assert!(matches!(
            f[0].message,
            Msg::ImpossibleCondition {
                var_id: 5,
                result: false,
                ..
            }
        ));
    }

    #[test]
    fn no_dead_branches_no_findings() {
        let ir = Ir::builder(Engine::Mz).finish();
        let ctx = RuleCtx::new(&ir);
        assert!(ImpossibleCondition.run(&ctx).is_empty());
    }
}
