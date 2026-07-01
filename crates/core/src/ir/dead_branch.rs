//! The "constant-resolvable condition" fact (dead branch) — input for the
//! `impossible-condition` rule.
//!
//! Using a light **symbolic-range** analysis over command 122 (Control
//! Variables: set/add/sub/random) within a single command list, the adapter
//! determines that a command 111 condition is always true or always false, and
//! places the **ready verdict** here. Both sides of the comparison are carried as
//! inclusive integer ranges (`[lo, hi]`); an exact constant is the degenerate
//! range `lo == hi`. The core merely turns it into a finding — it does not know
//! the semantics of codes 111/122 itself (that lives in the adapter, like all
//! other RPG-Maker specifics).

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
/// Both operands are inclusive integer ranges derived by the adapter's symbolic
/// propagation (`value_lo..=value_hi` for the left variable, `operand_lo..=
/// operand_hi` for the right side). An exact value is `lo == hi`. `result` — what
/// the condition is guaranteed to evaluate to over those ranges: `true` → the
/// "else" branch is unreachable, `false` → the "then" branch is unreachable.
#[derive(Clone, Debug, serde::Serialize)]
pub struct DeadBranch {
    /// Location of command 111.
    pub location: Location,
    /// Id of the variable from the condition.
    pub var_id: u32,
    /// Lower bound of the variable's propagated value range.
    pub value_lo: i64,
    /// Upper bound of the variable's propagated value range.
    pub value_hi: i64,
    /// Comparison operator.
    pub op: CmpOp,
    /// Lower bound of the right operand's value range.
    pub operand_lo: i64,
    /// Upper bound of the right operand's value range.
    pub operand_hi: i64,
    /// Guaranteed result of the condition over the ranges.
    pub result: bool,
}
