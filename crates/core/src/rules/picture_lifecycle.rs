//! Rule `picture-lifecycle`: a picture operated on before it is shown.
//!
//! Consumes the adapter's facts ([`crate::ir::Ir::picture_misuses`]): a
//! Move/Rotate/Tint/Erase Picture command that runs before the matching Show
//! Picture on the same straight-line command sequence. The operation targets a
//! picture that does not exist yet — it is a no-op / an ordering mistake.
//!
//! The adapter only emits a fact when the op and the show are on the same
//! execution path (same indent, no branch boundary between them) and the op is not
//! inside a loop body (where the picture persists across iterations), so the
//! cross-branch and loop-redraw false positives are avoided. Confidence `likely`
//! and **off by default** (opt-in via `--pictures`): a picture can still be shown by
//! a prior event/script that persists across command lists, which static analysis
//! cannot follow.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};

/// Rule that flags picture operations preceding the picture's Show.
pub struct PictureLifecycle;

impl Rule for PictureLifecycle {
    fn id(&self) -> &'static str {
        "picture-lifecycle"
    }

    fn category(&self) -> Category {
        Category::DeadCode
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        ctx.ir
            .picture_misuses
            .iter()
            .map(|m| Finding {
                severity: Severity::Warning,
                category: Category::DeadCode,
                confidence: Confidence::Likely,
                location: m.location.clone(),
                message: Msg::PictureBeforeShow {
                    picture_id: m.picture_id,
                    op: m.op,
                },
                references: Vec::new(),
                rule: "picture-lifecycle",
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Engine, Ir, Location, PictureMisuse, PictureOp};

    #[test]
    fn emits_finding_per_misuse() {
        let mut b = Ir::builder(Engine::Mz);
        b.add_picture_misuse(PictureMisuse {
            picture_id: 2,
            op: PictureOp::Move,
            location: Location::file_only("data/Map001.json"),
        });
        let ir = b.finish();
        let f = PictureLifecycle.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(f[0].confidence, Confidence::Likely);
        assert!(matches!(
            f[0].message,
            Msg::PictureBeforeShow {
                picture_id: 2,
                op: PictureOp::Move
            }
        ));
    }

    #[test]
    fn no_facts_no_findings() {
        let ir = Ir::builder(Engine::Mz).finish();
        assert!(PictureLifecycle.run(&RuleCtx::new(&ir)).is_empty());
    }
}
