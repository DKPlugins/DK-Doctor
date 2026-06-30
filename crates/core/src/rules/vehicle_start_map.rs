//! Rule `vehicle-start-map`: the vehicle's start map does not exist.
//!
//! `System.json` defines boat/ship/airship.startMapId — the map where the vehicle
//! sits at start. If id != 0 but no map with that id exists in the project
//! (`maps_by_id`), the vehicle won't appear and can't be boarded (dead content) —
//! NOT a crash: the engine only compares startMapId with the current map, it does
//! not load that map separately (verified against `rmmz_objects.js`). 0 = "unset",
//! skipped.
//!
//! Confidence `likely`, severity `warning`: the vehicle may be deliberately
//! relocated at runtime via the "Set Vehicle Location" command (202), which static
//! analysis does not track here (the same honest-disclaimer approach as in
//! `unreachable-maps`/`uninitialized-symbols`).

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};

/// Rule that checks vehicle start maps.
pub struct VehicleStartMap;

impl Rule for VehicleStartMap {
    fn id(&self) -> &'static str {
        "vehicle-start-map"
    }
    fn category(&self) -> Category {
        Category::Reference
    }
    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        for v in &ctx.ir.vehicle_start_maps {
            // map_id != 0 is already guaranteed by the adapter, but play it safe.
            if v.map_id == 0 || ctx.ir.maps_by_id.contains_key(&v.map_id) {
                continue;
            }
            findings.push(Finding {
                severity: Severity::Warning,
                category: Category::Reference,
                confidence: Confidence::Likely,
                location: v.location.clone(),
                message: Msg::VehicleStartMapMissing {
                    vehicle: v.vehicle,
                    map_id: v.map_id,
                },
                references: Vec::new(),
                rule: "vehicle-start-map",
            });
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Engine, Entity, Ir, Location, Map, VehicleKind};

    fn ir_with(maps: &[u32], vehicles: &[(VehicleKind, u32)]) -> Ir {
        let mut b = Ir::builder(Engine::Mz);
        for &id in maps {
            b.push_entity(
                Entity::Map(Map {
                    map_id: id,
                    name: format!("M{id}"),
                    event_ids: vec![],
                    can_battle: false,
                }),
                Location::file_only(format!("data/Map{id:03}.json")),
            );
        }
        for &(k, mid) in vehicles {
            b.add_vehicle_start_map(k, mid, Location::file_only("data/System.json"));
        }
        b.finish()
    }

    #[test]
    fn flags_missing_vehicle_start_map_only() {
        // ship -> existing map 1 (ok); airship -> missing map 99 (warning/likely).
        let ir = ir_with(&[1], &[(VehicleKind::Ship, 1), (VehicleKind::Airship, 99)]);
        let f = VehicleStartMap.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(f[0].confidence, Confidence::Likely);
        assert!(matches!(
            f[0].message,
            Msg::VehicleStartMapMissing {
                vehicle: VehicleKind::Airship,
                map_id: 99
            }
        ));
    }

    #[test]
    fn flags_missing_boat_start_map() {
        // boat -> missing map 42 (covers the Boat label path).
        let ir = ir_with(&[1], &[(VehicleKind::Boat, 42)]);
        let f = VehicleStartMap.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert!(matches!(
            f[0].message,
            Msg::VehicleStartMapMissing {
                vehicle: VehicleKind::Boat,
                map_id: 42
            }
        ));
    }

    #[test]
    fn unset_and_existing_produce_nothing() {
        // No vehicle facts (all unset filtered by adapter) and an existing target.
        let ir = ir_with(&[1, 2], &[(VehicleKind::Boat, 2)]);
        assert!(VehicleStartMap.run(&RuleCtx::new(&ir)).is_empty());
    }
}
