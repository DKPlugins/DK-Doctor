//! Rule `db-reachability`: a database record nothing references.
//!
//! For a small set of DB kinds where "used" has a clear static meaning
//! (enemies, skills, weapons, armors), collects every id targeted by a
//! [`Edge::ReferencesDbId`] edge and flags records whose id appears in none.
//! Those channels already cover the structural references the adapter emits:
//! troop membership / Enemy Transform (enemies); class learnings, traits/effects,
//! enemy actions, Change Skill (skills); actor equips, shop goods, enemy drops,
//! Change Equipment, equip conditions (weapons/armors).
//!
//! Confidence `likely`, severity `info`, off by default (opt-in via
//! `--db-reachability`): plugins and notetags can reference records in ways static
//! analysis cannot see, so an "unused" record is a hint, not a certainty.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::{DbKind, Edge, Entity};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};
use rustc_hash::{FxHashMap, FxHashSet};

/// DB kinds whose reachability is checked (others have plugin/notetag-heavy or
/// ambiguous usage that would produce too many false positives).
const CHECKED: &[DbKind] = &[DbKind::Enemy, DbKind::Skill, DbKind::Weapon, DbKind::Armor];

/// Rule that flags DB records referenced nowhere in the data.
pub struct DbReachability;

impl Rule for DbReachability {
    fn id(&self) -> &'static str {
        "db-reachability"
    }

    fn category(&self) -> Category {
        Category::DeadCode
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        // Entity id of each DB record, so a record's own outgoing edges do not
        // count as "referenced" (a skill whose effect adds itself is still unused).
        let mut record_entity: FxHashMap<(DbKind, u32), crate::ir::EntityId> = FxHashMap::default();
        for node in &ctx.ir.entities {
            if let Entity::DatabaseRecord(r) = &node.kind {
                record_entity.insert((r.kind, r.record_id), node.id);
            }
        }
        // Ids referenced by any ReferencesDbId edge, keyed by (kind, id) — excluding
        // self-references (an edge from the very record it targets).
        let mut referenced: FxHashSet<(DbKind, u32)> = FxHashSet::default();
        for rec in &ctx.ir.edges {
            if let Edge::ReferencesDbId { kind, id } = rec.edge
                && record_entity.get(&(kind, id)) != Some(&rec.from)
            {
                referenced.insert((kind, id));
            }
        }

        let mut findings = Vec::new();
        for node in &ctx.ir.entities {
            let Entity::DatabaseRecord(record) = &node.kind else {
                continue;
            };
            if !CHECKED.contains(&record.kind) {
                continue;
            }
            if record.record_id == 0 {
                continue;
            }
            if referenced.contains(&(record.kind, record.record_id)) {
                continue;
            }
            findings.push(Finding {
                severity: Severity::Info,
                category: Category::DeadCode,
                confidence: Confidence::Likely,
                location: node.location.clone(),
                message: Msg::UnusedDbRecord {
                    kind: record.kind,
                    id: record.record_id,
                    name: (!record.name.is_empty()).then(|| record.name.clone()),
                },
                references: Vec::new(),
                rule: "db-reachability",
            });
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{DatabaseRecord, Engine, Ir, IrBuilder, Location};

    fn push_record(b: &mut IrBuilder, kind: DbKind, id: u32, name: &str) {
        b.push_entity(
            Entity::DatabaseRecord(DatabaseRecord {
                kind,
                record_id: id,
                name: name.to_string(),
            }),
            Location::file_only("data/Skills.json"),
        );
    }

    #[test]
    fn flags_only_unreferenced_checked_kinds() {
        let mut b = Ir::builder(Engine::Mz);
        // Skill 1 referenced (e.g. class learning), skill 2 not.
        push_record(&mut b, DbKind::Skill, 1, "Fire");
        push_record(&mut b, DbKind::Skill, 2, "Unused");
        // Enemy 5 referenced by a troop member, enemy 6 not.
        push_record(&mut b, DbKind::Enemy, 5, "Slime");
        push_record(&mut b, DbKind::Enemy, 6, "Ghost");
        // Item 9 is NOT a checked kind — never flagged even without references.
        push_record(&mut b, DbKind::Item, 9, "Potion");
        let from = b.push_entity(
            Entity::DatabaseRecord(DatabaseRecord {
                kind: DbKind::Class,
                record_id: 1,
                name: "Hero".to_string(),
            }),
            Location::file_only("data/Classes.json"),
        );
        b.push_edge(
            from,
            Edge::ReferencesDbId {
                kind: DbKind::Skill,
                id: 1,
            },
            Location::file_only("data/Classes.json"),
        );
        b.push_edge(
            from,
            Edge::ReferencesDbId {
                kind: DbKind::Enemy,
                id: 5,
            },
            Location::file_only("data/Troops.json"),
        );
        let ir = b.finish();
        let f = DbReachability.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|x| x.severity == Severity::Info));
        assert!(f.iter().any(|x| matches!(
            x.message,
            Msg::UnusedDbRecord {
                kind: DbKind::Skill,
                id: 2,
                ..
            }
        )));
        assert!(f.iter().any(|x| matches!(
            x.message,
            Msg::UnusedDbRecord {
                kind: DbKind::Enemy,
                id: 6,
                ..
            }
        )));
    }

    #[test]
    fn self_reference_does_not_count_as_used() {
        // Skill 5's own effect adds skill 5 (a self-edge). Nothing else references
        // it → still flagged as unused (the self-edge must not mask it).
        let mut b = Ir::builder(Engine::Mz);
        let skill5 = b.push_entity(
            Entity::DatabaseRecord(DatabaseRecord {
                kind: DbKind::Skill,
                record_id: 5,
                name: "Loop".to_string(),
            }),
            Location::file_only("data/Skills.json"),
        );
        b.push_edge(
            skill5,
            Edge::ReferencesDbId {
                kind: DbKind::Skill,
                id: 5,
            },
            Location::file_only("data/Skills.json"),
        );
        let ir = b.finish();
        let f = DbReachability.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert!(matches!(
            f[0].message,
            Msg::UnusedDbRecord {
                kind: DbKind::Skill,
                id: 5,
                ..
            }
        ));
    }

    #[test]
    fn empty_project_yields_nothing() {
        let ir = Ir::builder(Engine::Mz).finish();
        assert!(DbReachability.run(&RuleCtx::new(&ir)).is_empty());
    }
}
