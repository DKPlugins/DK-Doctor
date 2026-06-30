//! Rule `referential-integrity`: a reference to a DB record that does not exist.
//!
//! Each [`Edge::ReferencesDbId`] edge is checked via
//! [`Ir::db_exists`](crate::ir::Ir::db_exists). A missing record of the given
//! kind and id is a dangling reference (granting a nonexistent item, a battle
//! with a nonexistent enemy, and so on).

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::Edge;
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};

/// Rule that checks the referential integrity of DB records.
pub struct ReferentialIntegrity;

impl Rule for ReferentialIntegrity {
    fn id(&self) -> &'static str {
        "referential-integrity"
    }

    fn category(&self) -> Category {
        Category::Reference
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        for rec in &ctx.ir.edges {
            let Edge::ReferencesDbId { kind, id } = rec.edge else {
                continue;
            };
            if ctx.ir.db_exists(kind, id) {
                continue;
            }
            findings.push(Finding {
                severity: Severity::Error,
                category: Category::Reference,
                confidence: Confidence::Certain,
                location: rec.location.clone(),
                message: Msg::DanglingDbRef { kind, id },
                references: Vec::new(),
                rule: "referential-integrity",
            });
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{DatabaseRecord, DbKind, Engine, Entity, Ir, Location};

    #[test]
    fn flags_dangling_db_reference_only() {
        let mut b = Ir::builder(Engine::Mz);
        let rec = b.push_entity(
            Entity::DatabaseRecord(DatabaseRecord {
                kind: DbKind::Item,
                record_id: 5,
                name: "Potion".to_string(),
            }),
            Location::file_only("data/Items.json"),
        );
        // Reference to existing item 5 — fine.
        b.push_edge(
            rec,
            Edge::ReferencesDbId {
                kind: DbKind::Item,
                id: 5,
            },
            Location::file_only("data/Map001.json"),
        );
        // Reference to missing item 6 — error.
        b.push_edge(
            rec,
            Edge::ReferencesDbId {
                kind: DbKind::Item,
                id: 6,
            },
            Location::file_only("data/Map001.json"),
        );
        // Reference to missing enemy 2 — error (a different kind with no records).
        b.push_edge(
            rec,
            Edge::ReferencesDbId {
                kind: DbKind::Enemy,
                id: 2,
            },
            Location::file_only("data/Troops.json"),
        );
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = ReferentialIntegrity.run(&ctx);
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|x| x.severity == Severity::Error));
        assert!(f.iter().any(|x| matches!(
            x.message,
            Msg::DanglingDbRef {
                kind: DbKind::Item,
                id: 6
            }
        )));
        assert!(f.iter().any(|x| matches!(
            x.message,
            Msg::DanglingDbRef {
                kind: DbKind::Enemy,
                id: 2
            }
        )));
    }
}
