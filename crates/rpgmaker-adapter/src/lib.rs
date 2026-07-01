//! `dk-doctor-rpgmaker` — RPG Maker MV/MZ adapter: parses `data/*.json`,
//! interprets event command lists and builds [`Ir`].
//!
//! All RPG-Maker specifics (command codes, parameter indices, JSON parsing)
//! live here; the core stays engine-independent. The entry point is
//! [`load_project`]; command indices are verified against real MV/MZ data per
//! `docs/rpgmaker-format-spec.md`.

mod assets;
mod atlas;
mod build;
mod codes;
mod command;
mod db_edges;
mod interpreter;
mod plugins;
mod profiles;
mod raw;

use camino::Utf8Path;
use dk_doctor_core::ir::Ir;

pub use atlas::{
    AtlasEvent, CommandLine, MapAtlas, MapEdge, MapGraph, MapNode, MapRender, event_page_commands,
    map_atlas, map_graph, map_render, read_project_image,
};

/// Adapter error when loading a project.
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    /// Project root not found or does not contain a `data/` folder (including in `www/`).
    #[error("проект не найден или не содержит data/: {0}")]
    ProjectNotFound(String),

    /// The `data/` folder exists, but no core file parsed as JSON: the data is
    /// encrypted in a non-standard way (for example, `.KEL` in OMORI) or it is
    /// not an RPG Maker project. Analysis is impossible — we emit an explicit
    /// signal so the CLI does not print a misleading "0 problems / clean".
    #[error(
        "не найдено анализируемых данных RPG Maker (зашифровано или нестандартный формат?): {0}"
    )]
    NoAnalyzableData(String),

    /// Input/output error.
    #[error("ошибка ввода-вывода: {0}")]
    Io(#[from] std::io::Error),

    /// JSON parse error in a specific file.
    #[error("ошибка разбора JSON в {file}: {source}")]
    Json {
        /// The file on which the error occurred.
        file: String,
        /// The underlying serde_json error.
        source: serde_json::Error,
    },
}

impl AdapterError {
    /// Maps the error into a language-neutral `(kind, detail)` pair for
    /// localized display in the CLI/desktop ([`dk_doctor_core::Chrome::LoadError`]).
    /// The detail is a path / file name / system message, without pre-rendered text.
    pub fn to_load_error(&self) -> (dk_doctor_core::LoadErrorKind, String) {
        use dk_doctor_core::LoadErrorKind as K;
        match self {
            AdapterError::ProjectNotFound(path) => (K::NotFound, path.clone()),
            AdapterError::NoAnalyzableData(path) => (K::NotAnalyzable, path.clone()),
            AdapterError::Io(e) => (K::Io, e.to_string()),
            AdapterError::Json { file, source } => (K::ParseError, format!("{file}: {source}")),
        }
    }
}

/// Loads an RPG Maker project from the root folder and builds [`Ir`].
///
/// Tries the root itself, then `www/`. Parses `System.json`, the database,
/// common events, maps and enemy troops; interprets command lists into IR
/// edges and symbol sites; scans asset folders. Parse errors in individual
/// files are not fatal — they are skipped, the project is built from what is available.
pub fn load_project(root: &Utf8Path) -> Result<Ir, AdapterError> {
    let (ir, _warnings) = build::build(root)?;
    Ok(ir)
}

/// Like [`load_project`], but also returns the non-fatal load warnings — one
/// message per project file that could not be parsed and was skipped.
///
/// The CLI and desktop surface these so the user knows the report may be
/// incomplete (e.g. a corrupt `MapXXX.json`), instead of silently dropping them.
pub fn load_project_with_warnings(root: &Utf8Path) -> Result<(Ir, Vec<String>), AdapterError> {
    let (ir, warnings) = build::build(root)?;
    Ok((ir, warnings.messages))
}

/// Mines a plugin's source into a **curated-profile skeleton** (commented TOML)
/// for a human to review. Reuses the Tier-A/Tier-B analyzers; the game is not run.
/// `name` is the plugin name (`.js` file stem == `$plugins[].name`).
///
/// The skeleton emits only low-false-positive facts (commands, dependencies, asset
/// hints) as active tables; symbol/db params are left as commented guidance so a
/// mined profile never silently introduces a false alarm. Powers `xtask
/// mine-plugin-profile`.
pub fn mine_plugin_profile(name: &str, src: &str) -> String {
    plugins::mine::to_toml_skeleton(&plugins::mine::mine(name, src))
}

#[cfg(test)]
mod tests {
    use crate::command::EventCommand;
    use crate::interpreter::{WalkCtx, walk};
    use dk_doctor_core::ir::{
        AssetKind, DbKind, Edge, Engine, Ir, IrBuilder, PathSeg, TransferDesignation,
    };
    use serde_json::json;

    /// Builds a command list from JSON arrays `[code, indent, parameters]`.
    fn cmds(items: Vec<serde_json::Value>) -> Vec<EventCommand> {
        items
            .into_iter()
            .map(|v| serde_json::from_value(v).unwrap())
            .collect()
    }

    /// Runs the interpreter over the list and returns the populated builder.
    fn run(list: &[EventCommand]) -> IrBuilder {
        let mut b = Ir::builder(Engine::Mz);
        let entity = b.push_entity(
            dk_doctor_core::ir::Entity::Page(dk_doctor_core::ir::Page {
                conditions: Default::default(),
                trigger: dk_doctor_core::ir::PageTrigger::Action,
                command_count: list.len() as u32,
                commands: Vec::new(),
            }),
            dk_doctor_core::ir::Location::file_only("data/Map001.json"),
        );
        let ctx = WalkCtx {
            entity,
            file: "data/Map001.json".into(),
            base_path: vec![PathSeg::Map(1), PathSeg::Event(1), PathSeg::Page(1)],
            self_switch_scope: Some(crate::interpreter::SelfSwitchScope {
                map_id: 1,
                event_id: 1,
            }),
            gate_switches: Vec::new(),
        };
        walk(&mut b, &ctx, list);
        b
    }

    /// Like [`run`], but the command list executes behind the given global-switch
    /// activation gate (for `circular-gate` / `SwitchGate` emission tests).
    fn run_gated(list: &[EventCommand], gate: Vec<u32>) -> IrBuilder {
        let mut b = Ir::builder(Engine::Mz);
        let entity = b.push_entity(
            dk_doctor_core::ir::Entity::Page(dk_doctor_core::ir::Page {
                conditions: Default::default(),
                trigger: dk_doctor_core::ir::PageTrigger::Action,
                command_count: list.len() as u32,
                commands: Vec::new(),
            }),
            dk_doctor_core::ir::Location::file_only("data/Map001.json"),
        );
        let ctx = WalkCtx {
            entity,
            file: "data/Map001.json".into(),
            base_path: vec![PathSeg::Map(1), PathSeg::Event(1), PathSeg::Page(1)],
            self_switch_scope: Some(crate::interpreter::SelfSwitchScope {
                map_id: 1,
                event_id: 1,
            }),
            gate_switches: gate,
        };
        walk(&mut b, &ctx, list);
        b
    }

    #[test]
    fn transfer_direct_to_map_5() {
        // 201: designation 0 (direct) → map 5.
        let list = cmds(vec![
            json!({"code":201,"indent":0,"parameters":[0,5,8,12,0,0]}),
        ]);
        let ir = run(&list).finish();
        let transfers: Vec<_> = ir
            .edges
            .iter()
            .filter_map(|r| match &r.edge {
                Edge::Transfer {
                    to_map,
                    designation,
                } => Some((*to_map, *designation)),
                _ => None,
            })
            .collect();
        assert_eq!(transfers.len(), 1);
        assert_eq!(transfers[0].0, Some(5));
        assert!(matches!(transfers[0].1, TransferDesignation::Direct));
    }

    #[test]
    fn transfer_by_variable_reads_vars() {
        // 201: designation 1 (by variable) -> to_map None + 3 var reads.
        let list = cmds(vec![
            json!({"code":201,"indent":0,"parameters":[1,10,11,12,0,0]}),
        ]);
        let ir = run(&list).finish();
        let transfer = ir
            .edges
            .iter()
            .find_map(|r| match &r.edge {
                Edge::Transfer {
                    to_map,
                    designation,
                } => Some((*to_map, *designation)),
                _ => None,
            })
            .unwrap();
        assert_eq!(transfer.0, None);
        assert!(matches!(transfer.1, TransferDesignation::ByVariable));
        let reads = ir
            .edges
            .iter()
            .filter(|r| matches!(r.edge, Edge::ReadsVariable { .. }))
            .count();
        assert_eq!(reads, 3);
        assert_eq!(ir.symbols.variables.get(&10).unwrap().reads.len(), 1);
    }

    #[test]
    fn control_variables_range_writes_three() {
        // 122 over 4..6 inclusive → three WritesVariable.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[4,6,0,0,0]}),
        ]);
        let ir = run(&list).finish();
        let writes: Vec<u32> = ir
            .edges
            .iter()
            .filter_map(|r| match &r.edge {
                Edge::WritesVariable { variable_id } => Some(*variable_id),
                _ => None,
            })
            .collect();
        assert_eq!(writes, vec![4, 5, 6]);
        assert_eq!(ir.symbols.variables.len(), 3);
    }

    #[test]
    fn control_variables_operand_variable_reads_source() {
        // 122: operand type 1 → [4] srcVarId READ.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[1,1,0,1,20]}),
        ]);
        let ir = run(&list).finish();
        assert!(ir.symbols.variables.get(&1).unwrap().writes.len() == 1);
        assert!(ir.symbols.variables.get(&20).unwrap().reads.len() == 1);
    }

    /// Helper: the target map of the first Transfer edge (if any).
    fn first_transfer_to_map(ir: &Ir) -> Option<Option<u32>> {
        ir.edges.iter().find_map(|r| match &r.edge {
            Edge::Transfer { to_map, .. } => Some(*to_map),
            _ => None,
        })
    }

    #[test]
    fn transfer_by_variable_resolved_via_const_122() {
        // 122 var#10 = 42 (set, const), then 201 by variable [1]=10 ->
        // constant-propagation resolves map 42.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[10,10,0,0,42]}),
            json!({"code":201,"indent":0,"parameters":[1,10,11,12,0,0]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(first_transfer_to_map(&ir), Some(Some(42)));
    }

    #[test]
    fn battle_by_variable_resolved_via_const_122() {
        // 122 var#5 = 7, then 301 by variable [1]=5 -> reference to troop #7.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[5,5,0,0,7]}),
            json!({"code":301,"indent":0,"parameters":[1,5,false,false]}),
        ]);
        let ir = run(&list).finish();
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Troop,
                id: 7
            }
        )));
    }

    #[test]
    fn change_equipment_references_actor_and_item() {
        let list = cmds(vec![
            // Slot 1 is weapon.
            json!({"code":319,"indent":0,"parameters":[3,1,7]}),
            // Other equip slots are armor.
            json!({"code":319,"indent":0,"parameters":[3,2,8]}),
        ]);
        let ir = run(&list).finish();
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Actor,
                id: 3
            }
        )));
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Weapon,
                id: 7
            }
        )));
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Armor,
                id: 8
            }
        )));
    }

    #[test]
    fn impossible_condition_const_eval() {
        // 122 var#1 = 10, then 111 type1 "var#1 == 5" -> always false.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[1,1,0,0,10]}),
            json!({"code":111,"indent":0,"parameters":[1,1,0,5,0]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(ir.dead_branches.len(), 1);
        let db = &ir.dead_branches[0];
        assert_eq!(db.var_id, 1);
        assert_eq!((db.value_lo, db.value_hi), (10, 10));
        assert_eq!((db.operand_lo, db.operand_hi), (5, 5));
        assert!(!db.result, "10 == 5 ложно");
    }

    #[test]
    fn random_range_makes_equality_dead() {
        // PR-7: 122 var#1 = random 1..3, then 111 "var#1 == 5" → out of range → always false.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[1,1,0,2,1,3]}),
            json!({"code":111,"indent":0,"parameters":[1,1,0,5,0]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(ir.dead_branches.len(), 1);
        let db = &ir.dead_branches[0];
        assert_eq!((db.value_lo, db.value_hi), (1, 3));
        assert!(!db.result, "5 вне диапазона 1..3 → ложно");
    }

    #[test]
    fn random_range_undecidable_is_not_flagged() {
        // Random 1..10, then "var#1 == 5": 5 IS inside the range → not statically
        // decidable → no dead branch.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[1,1,0,2,1,10]}),
            json!({"code":111,"indent":0,"parameters":[1,1,0,5,0]}),
        ]);
        let ir = run(&list).finish();
        assert!(ir.dead_branches.is_empty(), "5 внутри 1..10 → неопределимо");
    }

    #[test]
    fn add_shifts_range_out_of_bounds() {
        // PR-7: var#1 = 3, then += 4 (op 1), then "var#1 < 5" → range [7,7] → always false.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[1,1,0,0,3]}),
            json!({"code":122,"indent":0,"parameters":[1,1,1,0,4]}),
            json!({"code":111,"indent":0,"parameters":[1,1,0,5,4]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(ir.dead_branches.len(), 1);
        let db = &ir.dead_branches[0];
        assert_eq!((db.value_lo, db.value_hi), (7, 7));
        assert!(!db.result, "7 < 5 ложно");
    }

    #[test]
    fn add_to_random_range_resolves_ge() {
        // random 1..3, then += 10 → [11,13], then "var#1 >= 5" → always true.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[1,1,0,2,1,3]}),
            json!({"code":122,"indent":0,"parameters":[1,1,1,0,10]}),
            json!({"code":111,"indent":0,"parameters":[1,1,0,5,1]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(ir.dead_branches.len(), 1);
        let db = &ir.dead_branches[0];
        assert_eq!((db.value_lo, db.value_hi), (11, 13));
        assert!(db.result, ">= 5 истинно → мёртвая ветка «иначе»");
    }

    #[test]
    fn sub_below_operand_resolves_lt() {
        // var#1 = 3, then -= 10 (op 2) → [-7,-7], then "var#1 < 0" → always true.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[1,1,0,0,3]}),
            json!({"code":122,"indent":0,"parameters":[1,1,2,0,10]}),
            json!({"code":111,"indent":0,"parameters":[1,1,0,0,4]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(ir.dead_branches.len(), 1);
        let db = &ir.dead_branches[0];
        assert_eq!((db.value_lo, db.value_hi), (-7, -7));
        assert!(db.result, "-7 < 0 истинно");
    }

    #[test]
    fn mul_invalidates_range() {
        // var#1 = 3, then *= 2 (op 3, unsupported) → range unknown → no dead branch.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[1,1,0,0,3]}),
            json!({"code":122,"indent":0,"parameters":[1,1,3,0,2]}),
            json!({"code":111,"indent":0,"parameters":[1,1,0,100,0]}),
        ]);
        let ir = run(&list).finish();
        assert!(ir.dead_branches.is_empty(), "умножение гасит диапазон");
    }

    #[test]
    fn add_with_unknown_current_invalidates() {
        // += 4 on a variable with no known prior value → unknown, no dead branch.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[1,1,1,0,4]}),
            json!({"code":111,"indent":0,"parameters":[1,1,0,5,4]}),
        ]);
        let ir = run(&list).finish();
        assert!(
            ir.dead_branches.is_empty(),
            "нет исходного значения → неизвестно"
        );
    }

    #[test]
    fn switch_on_emits_gate_with_page_switches() {
        // PR-8: 121 sets switch #7 ON behind gate {3}. A SwitchGate is emitted.
        let list = cmds(vec![json!({"code":121,"indent":0,"parameters":[7,7,0]})]);
        let ir = run_gated(&list, vec![3]).finish();
        assert_eq!(ir.switch_gates.len(), 1);
        assert_eq!(ir.switch_gates[0].switch_id, 7);
        assert_eq!(ir.switch_gates[0].gate, vec![3]);
    }

    #[test]
    fn switch_off_emits_no_gate() {
        // 121 OFF (value 1) does not "provide" the switch → no SwitchGate, but marks ever_set_off.
        let list = cmds(vec![json!({"code":121,"indent":0,"parameters":[7,7,1]})]);
        let ir = run_gated(&list, vec![3]).finish();
        assert!(ir.switch_gates.is_empty(), "OFF-запись не даёт SwitchGate");
        assert!(ir.symbols.switches.get(&7).unwrap().ever_set_off);
    }

    #[test]
    fn ungated_switch_on_emits_empty_gate() {
        // Set ON with no activation gate → empty gate = freely settable.
        let list = cmds(vec![json!({"code":121,"indent":0,"parameters":[7,7,0]})]);
        let ir = run_gated(&list, Vec::new()).finish();
        assert_eq!(ir.switch_gates.len(), 1);
        assert!(ir.switch_gates[0].gate.is_empty());
    }

    #[test]
    fn const_cleared_after_common_event_call() {
        // 122 var#1 = 5, then 117 (CE call — opaque), then 201 by
        // variable [1]=1 -> environment is reset, the map is not resolved.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[1,1,0,0,5]}),
            json!({"code":117,"indent":0,"parameters":[3]}),
            json!({"code":201,"indent":0,"parameters":[1,1,2,3,0,0]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(first_transfer_to_map(&ir), Some(None));
    }

    #[test]
    fn loop_back_edge_invalidates_reassigned_const() {
        // Regression: var#1=10 before the loop, in the body condition "var#1==10", then var#1=99.
        // On 2+ iterations var#1=99 -> the branch is live. There should be no dead branch.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[1,1,0,0,10]}),
            json!({"code":112,"indent":0,"parameters":[]}),
            json!({"code":111,"indent":1,"parameters":[1,1,0,10,0]}),
            json!({"code":122,"indent":1,"parameters":[1,1,0,0,99]}),
            json!({"code":413,"indent":0,"parameters":[]}),
        ]);
        let ir = run(&list).finish();
        assert!(
            ir.dead_branches.is_empty(),
            "переменная меняется в теле цикла → ветка не мёртвая"
        );
    }

    #[test]
    fn loop_const_not_reassigned_still_resolves() {
        // Control: var#1=10 before the loop, in the body "var#1==10", WITHOUT reassignment ->
        // the variable is constant across all iterations -> the branch is dead (resolved).
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[1,1,0,0,10]}),
            json!({"code":112,"indent":0,"parameters":[]}),
            json!({"code":111,"indent":1,"parameters":[1,1,0,5,0]}),
            json!({"code":413,"indent":0,"parameters":[]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(ir.dead_branches.len(), 1, "константа цикла резолвится");
    }

    #[test]
    fn input_number_invalidates_const() {
        // Regression: 103 INPUT_NUMBER writes var#5 from player input -> the constant is cleared,
        // the subsequent "var#5==10" is not resolved.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[5,5,0,0,10]}),
            json!({"code":103,"indent":0,"parameters":[5,2]}),
            json!({"code":111,"indent":0,"parameters":[1,5,0,10,0]}),
        ]);
        let ir = run(&list).finish();
        assert!(
            ir.dead_branches.is_empty(),
            "ввод игрока гасит константу var#5"
        );
    }

    #[test]
    fn get_location_info_invalidates_const() {
        // Regression: 285 Get Location Info writes var#5 with map data -> the constant
        // is cleared, the by-variable transfer on var#5 is not resolved.
        let list = cmds(vec![
            json!({"code":122,"indent":0,"parameters":[5,5,0,0,42]}),
            json!({"code":285,"indent":0,"parameters":[5,0,0,3,4]}),
            json!({"code":201,"indent":0,"parameters":[1,5,6,7,0,0]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(first_transfer_to_map(&ir), Some(None));
    }

    #[test]
    fn const_in_branch_not_visible_after_block_closes() {
        // Dominance: 122 inside a branch (indent 1) is not visible after the
        // block closes (412 at indent 0) -> 201 at indent 0 is not resolved.
        let list = cmds(vec![
            json!({"code":111,"indent":0,"parameters":[0,9,0]}),
            json!({"code":122,"indent":1,"parameters":[10,10,0,0,42]}),
            json!({"code":412,"indent":0,"parameters":[]}),
            json!({"code":201,"indent":0,"parameters":[1,10,11,12,0,0]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(first_transfer_to_map(&ir), Some(None));
    }

    #[test]
    fn control_switches_range_writes() {
        // 121 over 5..5 → one switch write.
        let list = cmds(vec![json!({"code":121,"indent":0,"parameters":[5,5,0]})]);
        let ir = run(&list).finish();
        assert_eq!(ir.symbols.switches.get(&5).unwrap().writes.len(), 1);
    }

    #[test]
    fn control_switches_garbage_range_is_clamped() {
        // A malformed range [1, 4_000_000_000] must not iterate billions of times
        // and hang: the span is capped at MAX_SYMBOL_RANGE, so this terminates
        // with a bounded number of switch writes.
        let list = cmds(vec![
            json!({"code":121,"indent":0,"parameters":[1, 4_000_000_000u64, 0]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(
            ir.symbols.switches.len() as u64,
            crate::interpreter::MAX_SYMBOL_RANGE + 1,
            "range clamped to MAX_SYMBOL_RANGE"
        );
    }

    #[test]
    fn conditional_branch_switch_read() {
        // 111 type 0 → switch READ.
        let list = cmds(vec![json!({"code":111,"indent":0,"parameters":[0,14,0]})]);
        let ir = run(&list).finish();
        assert_eq!(ir.symbols.switches.get(&14).unwrap().reads.len(), 1);
    }

    #[test]
    fn conditional_branch_variable_with_src_var() {
        // 111 type 1, [2]==1 → both [1] and [3] are var reads.
        let list = cmds(vec![
            json!({"code":111,"indent":0,"parameters":[1,7,1,8,0]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(ir.symbols.variables.get(&7).unwrap().reads.len(), 1);
        assert_eq!(ir.symbols.variables.get(&8).unwrap().reads.len(), 1);
    }

    #[test]
    fn common_event_call_edge() {
        // 117 → CallsCommonEvent.
        let list = cmds(vec![json!({"code":117,"indent":0,"parameters":[42]})]);
        let ir = run(&list).finish();
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::CallsCommonEvent {
                common_event_id: 42
            }
        )));
    }

    #[test]
    fn show_text_face_asset_ref() {
        // 101 → face asset ref.
        let list = cmds(vec![
            json!({"code":101,"indent":0,"parameters":["Actor1",0,0,2,""]}),
        ]);
        let ir = run(&list).finish();
        assert!(
            ir.asset_refs
                .iter()
                .any(|(k, _)| k.kind == AssetKind::Face && k.name == "Actor1")
        );
    }

    #[test]
    fn play_se_audio_ref() {
        // 250 → SE asset ref from audio object.
        let list = cmds(vec![
            json!({"code":250,"indent":0,"parameters":[{"name":"Move1","volume":90,"pitch":100,"pan":0}]}),
        ]);
        let ir = run(&list).finish();
        assert!(
            ir.asset_refs
                .iter()
                .any(|(k, _)| k.kind == AssetKind::Se && k.name == "Move1")
        );
    }

    #[test]
    fn battle_direct_troop_ref() {
        // 301 designation 0 → troop DB ref.
        let list = cmds(vec![
            json!({"code":301,"indent":0,"parameters":[0,3,false,false]}),
        ]);
        let ir = run(&list).finish();
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Troop,
                id: 3
            }
        )));
    }

    #[test]
    fn change_items_db_ref() {
        // 126 → item DB ref.
        let list = cmds(vec![json!({"code":126,"indent":0,"parameters":[7,0,0,1]})]);
        let ir = run(&list).finish();
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Item,
                id: 7
            }
        )));
    }

    #[test]
    fn change_actor_images_three_asset_refs() {
        // 322 → character + face + sv_actor refs.
        let list = cmds(vec![
            json!({"code":322,"indent":0,"parameters":[1,"Hero",0,"HeroFace",2,"HeroSv"]}),
        ]);
        let ir = run(&list).finish();
        assert!(
            ir.asset_refs
                .iter()
                .any(|(k, _)| k.kind == AssetKind::Character && k.name == "Hero")
        );
        assert!(
            ir.asset_refs
                .iter()
                .any(|(k, _)| k.kind == AssetKind::Face && k.name == "HeroFace")
        );
        assert!(
            ir.asset_refs
                .iter()
                .any(|(k, _)| k.kind == AssetKind::SvActor && k.name == "HeroSv")
        );
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Actor,
                id: 1
            }
        )));
    }

    #[test]
    fn actor_ex_literal_vs_variable() {
        // 313 [0]==0 → actor DB ref + state ref; [0]==1 → var read.
        let direct = cmds(vec![json!({"code":313,"indent":0,"parameters":[0,2,0,5]})]);
        let ir = run(&direct).finish();
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Actor,
                id: 2
            }
        )));
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::State,
                id: 5
            }
        )));

        let by_var = cmds(vec![json!({"code":313,"indent":0,"parameters":[1,9,0,5]})]);
        let ir2 = run(&by_var).finish();
        assert_eq!(ir2.symbols.variables.get(&9).unwrap().reads.len(), 1);
    }

    #[test]
    fn tileset_change_db_ref() {
        // 282 → tileset DB ref (indirect).
        let list = cmds(vec![json!({"code":282,"indent":0,"parameters":[4]})]);
        let ir = run(&list).finish();
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Tileset,
                id: 4
            }
        )));
    }

    #[test]
    fn battleback_and_parallax_refs() {
        // 283 → battleback1/2; 284 → parallax.
        let list = cmds(vec![
            json!({"code":283,"indent":0,"parameters":["Castle","Brick"]}),
            json!({"code":284,"indent":0,"parameters":["BlueSky",true,true,0,0]}),
        ]);
        let ir = run(&list).finish();
        assert!(
            ir.asset_refs
                .iter()
                .any(|(k, _)| k.kind == AssetKind::Battleback1 && k.name == "Castle")
        );
        assert!(
            ir.asset_refs
                .iter()
                .any(|(k, _)| k.kind == AssetKind::Battleback2 && k.name == "Brick")
        );
        assert!(
            ir.asset_refs
                .iter()
                .any(|(k, _)| k.kind == AssetKind::Parallax && k.name == "BlueSky")
        );
    }

    #[test]
    fn script_becomes_blackbox_entity_and_tier_b_extracts_write() {
        // 355 -> Script entity AND Tier B extracts a literal write of switch #1.
        let list = cmds(vec![
            json!({"code":355,"indent":0,"parameters":["$gameSwitches.setValue(1, true);"]}),
        ]);
        let ir = run(&list).finish();
        assert!(
            ir.entities
                .iter()
                .any(|n| matches!(n.kind, dk_doctor_core::ir::Entity::Script(_)))
        );
        // Tier B: switch #1 now has a write site (needed for stuck-autorun/uninit).
        assert_eq!(ir.symbols.switches.get(&1).unwrap().writes.len(), 1);
        assert!(
            ir.edges
                .iter()
                .any(|r| matches!(r.edge, Edge::WritesSwitch { switch_id: 1 }))
        );
    }

    #[test]
    fn multiline_script_block_concatenates_355_655() {
        // 355 + 655: the body is assembled into a block; the switch write from the second line
        // is extracted (var/self-switch from scripts are intentionally not emitted).
        let list = cmds(vec![
            json!({"code":355,"indent":0,"parameters":["if (cond) {"]}),
            json!({"code":655,"indent":0,"parameters":["  $gameSwitches.setValue(7, false);"]}),
            json!({"code":655,"indent":0,"parameters":["}"]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(ir.symbols.switches.get(&7).unwrap().writes.len(), 1);
        // One blackbox for the whole block (not three).
        let scripts = ir
            .entities
            .iter()
            .filter(|n| matches!(n.kind, dk_doctor_core::ir::Entity::Script(_)))
            .count();
        assert_eq!(scripts, 1);
    }

    #[test]
    fn script_foreign_self_switch_write_not_registered() {
        // A FOREIGN event id (literal 9, not this._eventId) is NOT bound: we cannot
        // attribute it to a known event → the self-switch table stays untouched
        // (avoids the cross-event false dead-self-switch seen on the corpus).
        let list = cmds(vec![
            json!({"code":355,"indent":0,"parameters":["$gameSelfSwitches.setValue([this._mapId, 9, 'C'], true);"]}),
        ]);
        let ir = run(&list).finish();
        assert!(ir.self_switches.entries.is_empty());
    }

    #[test]
    fn script_current_event_self_switch_write_bound_to_scope() {
        use dk_doctor_core::ir::SelfSwitchKey;
        // The CURRENT-EVENT idiom binds to the script's own event scope (map1, ev1).
        let list = cmds(vec![
            json!({"code":355,"indent":0,"parameters":["$gameSelfSwitches.setValue([this._mapId, this._eventId, 'C'], true);"]}),
        ]);
        let ir = run(&list).finish();
        let info = ir
            .self_switches
            .entries
            .get(&SelfSwitchKey::new(1, 1, 'C'))
            .expect("self-switch C bound to current event");
        assert_eq!(info.writes.len(), 1);
        assert!(info.reads.is_empty());
    }

    #[test]
    fn control_switch_off_marks_ever_set_off() {
        // 121 with value OFF (1) marks ever_set_off; ON (0) does not.
        let list = cmds(vec![
            json!({"code":121,"indent":0,"parameters":[5,5,1]}),
            json!({"code":121,"indent":0,"parameters":[6,6,0]}),
        ]);
        let ir = run(&list).finish();
        assert!(ir.symbols.switches.get(&5).unwrap().ever_set_off);
        assert!(!ir.symbols.switches.get(&6).unwrap().ever_set_off);
    }

    #[test]
    fn self_switch_not_in_global_table() {
        // 123 → no global switch site.
        let list = cmds(vec![json!({"code":123,"indent":0,"parameters":["A",0]})]);
        let ir = run(&list).finish();
        assert!(ir.symbols.switches.is_empty());
    }

    #[test]
    fn self_switch_write_and_read_sites() {
        use dk_doctor_core::ir::SelfSwitchKey;
        // 123 ch A → WRITE; 111 type 2 ch A → READ; both keyed (map1,ev1,'A').
        let list = cmds(vec![
            json!({"code":123,"indent":0,"parameters":["A",0]}),
            json!({"code":111,"indent":0,"parameters":[2,"A",0]}),
        ]);
        let ir = run(&list).finish();
        // Global symbol table untouched.
        assert!(ir.symbols.switches.is_empty());
        let key = SelfSwitchKey::new(1, 1, 'A');
        let info = ir.self_switches.entries.get(&key).expect("self-switch A");
        assert_eq!(info.writes.len(), 1);
        assert_eq!(info.reads.len(), 1);
    }

    #[test]
    fn db_edge_dangling_skill_animation_fk() {
        use crate::raw::database::{Effect, Skill};
        use dk_doctor_core::ir::{DatabaseRecord, Edge, Engine, Entity, Ir, Location, PathSeg};
        // Skill with animationId 7 and effect 21 (ADD_STATE) → State 9.
        let skill = Skill {
            id: 1,
            name: "Fire".into(),
            animation_id: 7,
            damage: Default::default(),
            effects: vec![Effect {
                code: 21,
                data_id: 9,
            }],
        };
        let mut b = Ir::builder(Engine::Mz);
        let from = b.push_entity(
            Entity::DatabaseRecord(DatabaseRecord {
                kind: DbKind::Skill,
                record_id: 1,
                name: "Fire".into(),
            }),
            Location::new(
                "data/Skills.json",
                vec![PathSeg::DbRecord {
                    file: "Skills",
                    id: 1,
                }],
            ),
        );
        let loc = Location::new(
            "data/Skills.json",
            vec![PathSeg::DbRecord {
                file: "Skills",
                id: 1,
            }],
        );
        crate::db_edges::skill(&mut b, from, &loc, &skill);
        let ir = b.finish();
        // No Animations/States loaded → both FKs are dangling.
        assert!(!ir.db_exists(DbKind::Animation, 7));
        assert!(!ir.db_exists(DbKind::State, 9));
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Animation,
                id: 7
            }
        )));
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::State,
                id: 9
            }
        )));
    }

    #[test]
    fn actor_equips_slot_mapping() {
        use crate::raw::database::Actor;
        use dk_doctor_core::ir::{DatabaseRecord, Edge, Engine, Entity, Ir, Location, PathSeg};
        // equips: slot0=weapon 3, slot1=armor 4, slot2=empty(0 skipped); class 2.
        let actor = Actor {
            id: 1,
            name: "Hero".into(),
            class_id: 2,
            equips: vec![3, 4, 0],
            face_name: String::new(),
            character_name: String::new(),
            battler_name: String::new(),
            traits: Vec::new(),
        };
        let mut b = Ir::builder(Engine::Mz);
        let from = b.push_entity(
            Entity::DatabaseRecord(DatabaseRecord {
                kind: DbKind::Actor,
                record_id: 1,
                name: "Hero".into(),
            }),
            Location::file_only("data/Actors.json"),
        );
        let loc = Location::new(
            "data/Actors.json",
            vec![PathSeg::DbRecord {
                file: "Actors",
                id: 1,
            }],
        );
        crate::db_edges::actor(&mut b, from, &loc, &actor);
        let ir = b.finish();
        let has = |kind, id| {
            ir.edges
                .iter()
                .any(|r| matches!(&r.edge, Edge::ReferencesDbId { kind: k, id: i } if *k == kind && *i == id))
        };
        assert!(has(DbKind::Class, 2));
        assert!(has(DbKind::Weapon, 3));
        assert!(has(DbKind::Armor, 4));
        // empty slot (0) emits nothing.
        let armor_refs = ir
            .edges
            .iter()
            .filter(|r| {
                matches!(
                    &r.edge,
                    Edge::ReferencesDbId {
                        kind: DbKind::Armor,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(armor_refs, 1);
    }

    #[test]
    fn common_event_registered_in_db_index() {
        use dk_doctor_core::ir::{CeTrigger, CommonEvent, Engine, Entity, Ir, Location};
        let mut b = Ir::builder(Engine::Mz);
        b.push_entity(
            Entity::CommonEvent(CommonEvent {
                id: 5,
                name: "Heal".into(),
                trigger: CeTrigger::None,
                command_count: 0,
            }),
            Location::file_only("data/CommonEvents.json"),
        );
        let ir = b.finish();
        // CommonEvent existence is queryable via db_exists for effect-44 FKs.
        assert!(ir.db_exists(DbKind::CommonEvent, 5));
        assert!(!ir.db_exists(DbKind::CommonEvent, 6));
    }

    #[test]
    fn show_picture_ref() {
        // 231 [1] → picture.
        let list = cmds(vec![
            json!({"code":231,"indent":0,"parameters":[1,"Logo",0,0,0,0,100,100,255,0]}),
        ]);
        let ir = run(&list).finish();
        assert!(
            ir.asset_refs
                .iter()
                .any(|(k, _)| k.kind == AssetKind::Picture && k.name == "Logo")
        );
    }

    #[test]
    fn play_movie_ref() {
        // 261 → movie.
        let list = cmds(vec![json!({"code":261,"indent":0,"parameters":["Intro"]})]);
        let ir = run(&list).finish();
        assert!(
            ir.asset_refs
                .iter()
                .any(|(k, _)| k.kind == AssetKind::Movie && k.name == "Intro")
        );
    }

    #[test]
    fn show_animation_db_ref() {
        // 212 [1] → animation.
        let list = cmds(vec![
            json!({"code":212,"indent":0,"parameters":[-1,3,true]}),
        ]);
        let ir = run(&list).finish();
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Animation,
                id: 3
            }
        )));
    }

    #[test]
    fn shop_goods_refs() {
        // 302 first row + 605 extra row → item/weapon DB refs.
        let list = cmds(vec![
            json!({"code":302,"indent":0,"parameters":[0,7,0,0,false]}),
            json!({"code":605,"indent":0,"parameters":[1,2,0,0,false]}),
        ]);
        let ir = run(&list).finish();
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Item,
                id: 7
            }
        )));
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Weapon,
                id: 2
            }
        )));
    }

    #[test]
    fn plugins_parse_filters_separators() {
        let text = r#"// header
var $plugins =
[
{"name":"PluginA","status":true,"description":"","parameters":{}},
{"name":"--------","status":false,"description":"","parameters":{}},
{"name":"PluginB","status":false,"description":"","parameters":{}}
];"#;
        let plugins = crate::raw::plugins::parse(text);
        assert_eq!(plugins.len(), 2);
        assert_eq!(plugins[0].name, "PluginA");
        assert!(plugins[0].status);
        assert_eq!(plugins[1].name, "PluginB");
    }

    #[test]
    fn normalize_filename_strips_ext_and_encryption() {
        use crate::assets::normalize_filename;
        assert_eq!(normalize_filename("Actor1.png"), "Actor1");
        assert_eq!(normalize_filename("Actor1.png_"), "Actor1");
        assert_eq!(normalize_filename("Town.rpgmvo"), "Town");
        assert_eq!(normalize_filename("$BigSheet.png"), "$BigSheet");
        assert_eq!(normalize_filename("magic.efkefc"), "magic");
    }

    // --- Variable reads via message-text escapes (\v[n]) ---

    #[test]
    fn collect_text_var_ids_cases() {
        use crate::interpreter::collect_text_var_ids;
        let mut out = Vec::new();
        collect_text_var_ids(r"HP: \v[7] / \V[8]", &mut out);
        assert_eq!(out, vec![7, 8], "both \\v and \\V, case-insensitive");

        out.clear();
        collect_text_var_ids(r"\v[3] then \v[3] again", &mut out);
        assert_eq!(out, vec![3], "de-duplicated within one string");

        out.clear();
        collect_text_var_ids(r"\v[\v[3]]", &mut out);
        assert_eq!(
            out,
            vec![3],
            "nested: inner literal only, outer index dynamic"
        );

        out.clear();
        collect_text_var_ids(r"\v[0] \v[] \vx plain", &mut out);
        assert!(out.is_empty(), "id 0 / malformed escapes ignored");
    }

    #[test]
    fn show_text_data_reads_escaped_variable() {
        // 101 Show Text + 401 body line embedding \v[7] twice → variable #7 read once.
        let list = cmds(vec![
            json!({"code":101,"indent":0,"parameters":["",0,0,2,""]}),
            json!({"code":401,"indent":0,"parameters":[r"Score \v[7] of \v[7]"]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(ir.symbols.variables.get(&7).unwrap().reads.len(), 1);
    }

    #[test]
    fn show_choices_read_escaped_variable() {
        // 102 choice labels array embeds \v[9].
        let list = cmds(vec![
            json!({"code":102,"indent":0,"parameters":[[r"Pay \v[9]", "No"],1,0,2,0]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(ir.symbols.variables.get(&9).unwrap().reads.len(), 1);
    }

    // --- Variable reads via "operand by variable" slots ---

    #[test]
    fn change_hp_operand_by_variable_reads() {
        // 311 [0,1,1,0,1,false]: operandType [3]==0 → constant, no operand read
        // (only the actor target slot is fixed). [3]==1 → [4] is a variableId READ.
        let constant = cmds(vec![
            json!({"code":311,"indent":0,"parameters":[0,1,0,0,30,false]}),
        ]);
        assert!(
            !run(&constant).finish().symbols.variables.contains_key(&30),
            "constant amount is not a variable read"
        );

        let by_var = cmds(vec![
            json!({"code":311,"indent":0,"parameters":[0,1,0,1,30,false]}),
        ]);
        let ir = run(&by_var).finish();
        assert_eq!(
            ir.symbols.variables.get(&30).unwrap().reads.len(),
            1,
            "operandType==1 → value slot [4] is a variable read"
        );
    }

    #[test]
    fn change_parameter_operand_by_variable_reads_shifted_slot() {
        // 317: [2]=paramId, [3]=op, [4]=operandType, [5]=value/varId.
        let list = cmds(vec![
            json!({"code":317,"indent":0,"parameters":[0,4,2,0,1,12]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(ir.symbols.variables.get(&12).unwrap().reads.len(), 1);
    }

    #[test]
    fn change_gold_and_items_operand_by_variable() {
        // 125 Change Gold [0]=op,[1]=operandType,[2]=value/varId.
        let gold = cmds(vec![json!({"code":125,"indent":0,"parameters":[0,1,15]})]);
        assert_eq!(
            run(&gold)
                .finish()
                .symbols
                .variables
                .get(&15)
                .unwrap()
                .reads
                .len(),
            1
        );
        // 126 Change Items [0]=itemId,[1]=op,[2]=operandType,[3]=value/varId.
        let items = cmds(vec![json!({"code":126,"indent":0,"parameters":[7,0,1,16]})]);
        let ir = run(&items).finish();
        assert_eq!(ir.symbols.variables.get(&16).unwrap().reads.len(), 1);
        // The item DB ref is still emitted alongside the operand read.
        assert!(ir.edges.iter().any(|r| matches!(
            r.edge,
            Edge::ReferencesDbId {
                kind: DbKind::Item,
                id: 7
            }
        )));
    }

    #[test]
    fn show_picture_position_by_variable_reads() {
        // 231 [3]=designation; ==1 → [4]/[5] are x/y variableIds READ.
        let list = cmds(vec![
            json!({"code":231,"indent":0,"parameters":[1,"Pic",0,1,20,21,100,100,255,0]}),
        ]);
        let ir = run(&list).finish();
        assert_eq!(ir.symbols.variables.get(&20).unwrap().reads.len(), 1);
        assert_eq!(ir.symbols.variables.get(&21).unwrap().reads.len(), 1);
    }

    #[test]
    fn change_enemy_hp_operand_by_variable_reads() {
        // 331 Change Enemy HP: [0]=enemyIndex,[1]=op,[2]=operandType,[3]=value/varId,
        // [4]=allowDeath. operateValue([1],[2],[3]) → [2]==1 → [3] is a variableId READ.
        let constant = cmds(vec![
            json!({"code":331,"indent":0,"parameters":[0,0,0,50,false]}),
        ]);
        assert!(!run(&constant).finish().symbols.variables.contains_key(&50));
        let by_var = cmds(vec![
            json!({"code":331,"indent":0,"parameters":[0,0,1,50,false]}),
        ]);
        let ir = run(&by_var).finish();
        assert_eq!(ir.symbols.variables.get(&50).unwrap().reads.len(), 1);
    }
}
