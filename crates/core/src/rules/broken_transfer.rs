//! Rule `broken-transfer`: a transfer (command 201) to a nonexistent map.
//!
//! An [`Edge::Transfer`] edge with a direct map reference (`Direct`) and an id
//! that is missing from `maps_by_id` → a transfer to nowhere: the game crashes
//! on trigger. Transfers by variable (`ByVariable`) are usually dynamic
//! (`to_map = None`) and are skipped — but if the adapter's lightweight
//! constant-propagation resolved the map id from a 122 literal (`to_map =
//! Some`), such a transfer is checked too, with `likely` confidence (the value
//! could change outside of static analysis).

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::Edge;
use crate::ir::edge::TransferDesignation;
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};

/// Rule that finds transfers to a missing map.
pub struct BrokenTransfer;

impl Rule for BrokenTransfer {
    fn id(&self) -> &'static str {
        "broken-transfer"
    }

    fn category(&self) -> Category {
        Category::Reference
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        for rec in &ctx.ir.edges {
            let Edge::Transfer {
                to_map: Some(id),
                designation,
            } = rec.edge
            else {
                continue;
            };
            // Map 0 is the RPG Maker "unset" sentinel — a Direct transfer left at
            // map 0 (or a constant-resolved 0) is not a reference to a missing map.
            // The ByVariable path already filters this; guard the Direct path too.
            if id == 0 {
                continue;
            }
            if ctx.ir.maps_by_id.contains_key(&id) {
                continue;
            }
            // Direct transfer — `certain`; one resolved from a variable —
            // `likely` (value propagated heuristically, could change outside
            // static analysis).
            let (message, confidence) = match designation {
                TransferDesignation::Direct => {
                    (Msg::BrokenTransfer { map_id: id }, Confidence::Certain)
                }
                TransferDesignation::ByVariable => {
                    (Msg::BrokenTransferVar { map_id: id }, Confidence::Likely)
                }
            };
            findings.push(Finding {
                severity: Severity::Error,
                category: Category::Reference,
                confidence,
                location: rec.location.clone(),
                message,
                references: Vec::new(),
                rule: "broken-transfer",
            });
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Engine, Entity, Ir, Location, Map};

    #[test]
    fn flags_missing_target_but_not_existing_or_by_variable() {
        let mut b = Ir::builder(Engine::Mz);
        let m1 = b.push_entity(
            Entity::Map(Map {
                map_id: 1,
                name: "Town".to_string(),
                event_ids: vec![],
                can_battle: false,
            }),
            Location::file_only("data/Map001.json"),
        );
        // Direct transfer to existing map 1 — fine.
        b.push_edge(
            m1,
            Edge::Transfer {
                to_map: Some(1),
                designation: TransferDesignation::Direct,
            },
            Location::file_only("data/Map001.json"),
        );
        // Direct transfer to missing map 99 — error.
        b.push_edge(
            m1,
            Edge::Transfer {
                to_map: Some(99),
                designation: TransferDesignation::Direct,
            },
            Location::file_only("data/Map001.json"),
        );
        // Transfer by variable — skipped.
        b.push_edge(
            m1,
            Edge::Transfer {
                to_map: None,
                designation: TransferDesignation::ByVariable,
            },
            Location::file_only("data/Map001.json"),
        );
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = BrokenTransfer.run(&ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Error);
        assert!(matches!(f[0].message, Msg::BrokenTransfer { map_id: 99 }));
    }

    #[test]
    fn ignores_direct_transfer_to_map_zero() {
        let mut b = Ir::builder(Engine::Mz);
        let m1 = b.push_entity(
            Entity::Map(Map {
                map_id: 1,
                name: "Town".to_string(),
                event_ids: vec![],
                can_battle: false,
            }),
            Location::file_only("data/Map001.json"),
        );
        // Direct transfer to map 0 (the "unset" sentinel) — not a broken reference.
        b.push_edge(
            m1,
            Edge::Transfer {
                to_map: Some(0),
                designation: TransferDesignation::Direct,
            },
            Location::file_only("data/Map001.json"),
        );
        let ir = b.finish();
        assert!(BrokenTransfer.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn resolved_by_variable_transfer_is_likely() {
        let mut b = Ir::builder(Engine::Mz);
        let m1 = b.push_entity(
            Entity::Map(Map {
                map_id: 1,
                name: "Town".to_string(),
                event_ids: vec![],
                can_battle: false,
            }),
            Location::file_only("data/Map001.json"),
        );
        // Transfer by variable, but the map id was resolved by
        // constant-propagation to 42 — no such map → error with likely
        // confidence.
        b.push_edge(
            m1,
            Edge::Transfer {
                to_map: Some(42),
                designation: TransferDesignation::ByVariable,
            },
            Location::file_only("data/Map001.json"),
        );
        let ir = b.finish();
        let ctx = RuleCtx::new(&ir);

        let f = BrokenTransfer.run(&ctx);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Error);
        assert_eq!(f[0].confidence, Confidence::Likely);
        assert!(matches!(
            f[0].message,
            Msg::BrokenTransferVar { map_id: 42 }
        ));
    }
}
