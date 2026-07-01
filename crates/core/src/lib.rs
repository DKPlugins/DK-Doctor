//! `dk-doctor-core` — engine-independent core of the analyzer.
//!
//! Contains the IR types (entity graph + typed edges + symbol table),
//! the [`Finding`], the [`Report`] and the rules engine ([`rules`]). The core
//! knows nothing about RPG Maker — command codes and parsing live in the adapter.

pub mod finding;
pub mod ir;
pub mod message;
pub mod remediation;
pub mod report;
pub mod rules;

pub use finding::{Category, Confidence, Finding, Severity};
pub use ir::{
    AssetKey, AssetKind, AssetRef, BlockedTile, BlockedTileKind, CeTrigger, CmpOp, CommandMeta,
    CommonEvent, CommonEventSummary, DatabaseRecord, DbKind, DeadBranch, Edge, EdgeRecord, Engine,
    Entity, EntityId, EntityNode, Event, Ir, IrBuilder, Location, LocationPath, Map, MethodPatch,
    Page, PageConditions, PathSeg, PictureMisuse, PictureOp, PluginCommand, PluginCommandCall,
    PluginMeta, PluginOrderDeps, ScriptBlackbox, Site, SwitchGate, SymbolInfo, SymbolTable,
    TransferDesignation, Troop, VehicleKind, VehicleStartMap,
};
pub use message::{
    Chrome, Lang, LoadErrorKind, Msg, PluginOrderTag, SymbolKind, render, render_chrome,
};
pub use remediation::{Fix, FixKind, Remediation, autofix, remediation};
pub use report::{Report, Summary};
pub use rules::{Registry, Rule, RuleCtx};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::symbols::Site;

    #[test]
    fn severity_sorts_error_first() {
        let mut sevs = vec![Severity::Info, Severity::Error, Severity::Warning];
        sevs.sort_by(|a, b| b.cmp(a));
        assert_eq!(
            sevs,
            vec![Severity::Error, Severity::Warning, Severity::Info]
        );
        assert!(Severity::Error > Severity::Warning);
        assert!(Severity::Warning > Severity::Info);
    }

    #[test]
    fn symbol_table_insert_and_query() {
        let mut t = SymbolTable::default();
        t.declare_switch(14, Some("Boss Defeated".to_string()));

        let site = Site {
            location: Location::file_only("data/Map001.json"),
            entity: EntityId(0),
        };
        t.add_switch_write(14, site.clone());
        t.add_switch_read(14, site);

        let info = t.switches.get(&14).expect("switch 14 present");
        assert_eq!(info.id, 14);
        assert_eq!(info.name.as_deref(), Some("Boss Defeated"));
        assert_eq!(info.writes.len(), 1);
        assert_eq!(info.reads.len(), 1);
        assert!(!info.declared_by_plugin);

        // A variable read site without a declaration creates an entry.
        t.add_variable_read(
            3,
            Site {
                location: Location::file_only("data/Map002.json"),
                entity: EntityId(1),
            },
        );
        assert_eq!(t.variables.get(&3).unwrap().reads.len(), 1);
        assert!(t.variables.get(&3).unwrap().name.is_none());
    }

    #[test]
    fn ir_builder_finalizes_indexes() {
        let mut b = Ir::builder(Engine::Mz);
        let map_ent = b.push_entity(
            Entity::Map(Map {
                map_id: 1,
                name: "Town".to_string(),
                event_ids: vec![],
                can_battle: false,
            }),
            Location::file_only("data/Map001.json"),
        );
        b.push_entity(
            Entity::DatabaseRecord(DatabaseRecord {
                kind: DbKind::Item,
                record_id: 5,
                name: "Potion".to_string(),
            }),
            Location::file_only("data/Items.json"),
        );
        b.push_edge(
            map_ent,
            Edge::ReferencesDbId {
                kind: DbKind::Item,
                id: 5,
            },
            Location::file_only("data/Map001.json"),
        );
        b.set_start_map(Some(1));

        let ir = b.finish();
        assert_eq!(ir.engine, Engine::Mz);
        assert_eq!(ir.maps_by_id.get(&1), Some(&map_ent));
        assert!(ir.db_exists(DbKind::Item, 5));
        assert!(!ir.db_exists(DbKind::Item, 6));
        assert_eq!(ir.edges_from(map_ent).count(), 1);
        assert_eq!(ir.start_map_id, Some(1));
    }

    #[test]
    fn report_sorts_and_counts() {
        let mk = |sev: Severity, file: &str| Finding {
            severity: sev,
            category: Category::Data,
            confidence: Confidence::Certain,
            location: Location::file_only(file),
            message: Msg::DeadVariable {
                id: 1,
                name: None,
                writes: 1,
            },
            references: vec![],
            rule: "test-rule",
        };
        let report = Report::new(vec![
            mk(Severity::Info, "data/B.json"),
            mk(Severity::Error, "data/A.json"),
            mk(Severity::Warning, "data/A.json"),
        ]);
        assert_eq!(report.summary.errors, 1);
        assert_eq!(report.summary.warnings, 1);
        assert_eq!(report.summary.infos, 1);
        assert_eq!(report.findings[0].severity, Severity::Error);
        assert_eq!(report.exit_code(), 2);
    }

    #[test]
    fn location_path_renders_breadcrumb() {
        let loc = Location::new(
            "data/Map003.json",
            vec![
                PathSeg::Map(3),
                PathSeg::Event(5),
                PathSeg::Page(2),
                PathSeg::Command(14),
            ],
        );
        assert_eq!(loc.path.to_string(), "Map003/EV005/page2/cmd14");
    }

    #[test]
    fn registry_has_all_builtin_rules() {
        let reg = Registry::with_builtin();
        let ids: Vec<&str> = reg.rule_ids().collect();
        assert_eq!(ids.len(), 25);
        assert!(ids.contains(&"dead-variables"));
        assert!(ids.contains(&"uninitialized-symbols"));
        assert!(ids.contains(&"broken-transfer"));
        assert!(ids.contains(&"impossible-condition"));
        assert!(ids.contains(&"unreachable-maps"));
        assert!(ids.contains(&"referential-integrity"));
        assert!(ids.contains(&"broken-assets"));
        assert!(ids.contains(&"orphan-assets"));
        assert!(ids.contains(&"dead-code-after-exit"));
        assert!(ids.contains(&"dead-self-switch"));
        assert!(ids.contains(&"unreachable-self-switch"));
        assert!(ids.contains(&"dead-common-event"));
        assert!(ids.contains(&"cyclic-common-events"));
        assert!(ids.contains(&"shadowed-page"));
        assert!(ids.contains(&"stuck-autorun"));
        assert!(ids.contains(&"plugin-load-order"));
        assert!(ids.contains(&"missing-base"));
        assert!(ids.contains(&"unknown-plugin-command"));
        assert!(ids.contains(&"plugin-conflict"));
        assert!(ids.contains(&"vehicle-start-map"));
        assert!(ids.contains(&"circular-gate"));
        assert!(ids.contains(&"picture-lifecycle"));
        assert!(ids.contains(&"empty-event-page"));
        assert!(ids.contains(&"blocked-tile"));
        assert!(ids.contains(&"db-reachability"));
    }

    #[test]
    fn empty_project_yields_no_findings() {
        let reg = Registry::with_builtin();
        let ir = Ir::builder(Engine::Mv).finish();
        let ctx = RuleCtx::new(&ir);
        assert!(reg.run_all(&ctx).is_empty());
    }
}
