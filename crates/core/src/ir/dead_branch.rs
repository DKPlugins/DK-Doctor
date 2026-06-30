//! The "constant-resolvable condition" fact (dead branch) — input for the
//! `impossible-condition` rule.
//!
//! Using light constant-propagation (command 122 literals within a single
//! command list), the adapter determines that a command 111 condition is always
//! true or always false, and places the **ready verdict** here. The core merely
//! turns it into a finding — it does not know the semantics of codes 111/122
//! itself (that lives in the adapter, like all other RPG-Maker specifics).

use crate::ir::location::Location;

/// Comparison operator of a variable-based condition — engine-independent.
///
/// The adapter maps the numeric `comparison` code of command 111 (type 1) here.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CmpOp {
    /// `==`
    Eq,
    /// `>=`
    Ge,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `<`
    Lt,
    /// `!=`
    Ne,
}

impl CmpOp {
    /// Computes `lhs <op> rhs`.
    pub fn eval(self, lhs: i64, rhs: i64) -> bool {
        match self {
            CmpOp::Eq => lhs == rhs,
            CmpOp::Ge => lhs >= rhs,
            CmpOp::Le => lhs <= rhs,
            CmpOp::Gt => lhs > rhs,
            CmpOp::Lt => lhs < rhs,
            CmpOp::Ne => lhs != rhs,
        }
    }

    /// Language-neutral operator symbol (`"=="`, `">="`, …).
    pub fn symbol(self) -> &'static str {
        match self {
            CmpOp::Eq => "==",
            CmpOp::Ge => ">=",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Lt => "<",
            CmpOp::Ne => "!=",
        }
    }
}

/// Constant-resolvable command 111 condition (variable-based).
///
/// `result` — what the condition is guaranteed to evaluate to for the given
/// (propagated) variable value: `true` → the "else" branch is unreachable,
/// `false` → the "then" branch is unreachable.
#[derive(Clone, Debug, serde::Serialize)]
pub struct DeadBranch {
    /// Location of command 111.
    pub location: Location,
    /// Id of the variable from the condition.
    pub var_id: u32,
    /// Propagated constant value of the variable.
    pub value: i64,
    /// Comparison operator.
    pub op: CmpOp,
    /// Right operand of the comparison (constant).
    pub operand: i64,
    /// Guaranteed result of the condition.
    pub result: bool,
}
