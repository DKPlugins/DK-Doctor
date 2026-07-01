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
                    value_lo: db.value_lo,
                    value_hi: db.value_hi,
                    op: db.op,
                    operand_lo: db.operand_lo,
                    operand_hi: db.operand_hi,
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
            value_lo: 0,
            value_hi: 0,
            op: CmpOp::Ge,
            operand_lo: 1,
            operand_hi: 1,
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
    fn renders_range_when_value_is_not_exact() {
        // A symbolic range (random 1..3) that makes `var == 5` always false.
        let mut b = Ir::builder(Engine::Mz);
        b.add_dead_branch(DeadBranch {
            location: Location::file_only("data/Map001.json"),
            var_id: 7,
            value_lo: 1,
            value_hi: 3,
            op: CmpOp::Eq,
            operand_lo: 5,
            operand_hi: 5,
            result: false,
        });
        let ir = b.finish();
        let f = ImpossibleCondition.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        let en = crate::message::render(&f[0].message, crate::message::Lang::En);
        assert!(en.contains("1..3"), "range rendered: {en}");
    }

    #[test]
    fn no_dead_branches_no_findings() {
        let ir = Ir::builder(Engine::Mz).finish();
        let ctx = RuleCtx::new(&ir);
        assert!(ImpossibleCondition.run(&ctx).is_empty());
    }
}
