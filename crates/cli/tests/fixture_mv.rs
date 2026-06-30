//! Integration test: running the analyzer on the synthetic MV fixture
//! `testdata/mv-fixture/`. Validates the MV-specific adapter paths that differ
//! from MZ: engine detection, the 356 (raw-string) plugin command vs MZ's 357,
//! the 101 Show Text without the MZ-only 5th `speakerName` parameter, and the
//! MV animation format (`frames` present → `animation1Name` asset).

use dk_doctor_core::ir::{AssetKind, Engine};
use dk_doctor_core::rules::broken_transfer::BrokenTransfer;
use dk_doctor_core::{Msg, Rule, RuleCtx};
use dk_doctor_rpgmaker::load_project;

/// Loads the MV fixture project.
fn load_mv() -> dk_doctor_core::Ir {
    let root = camino::Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("testdata")
        .join("mv-fixture");
    load_project(&root).expect("MV fixture should load")
}

#[test]
fn detects_mv_engine() {
    // Presence of js/rpg_objects.js (no effects/, no 357) → MV.
    assert_eq!(load_mv().engine, Engine::Mv);
}

#[test]
fn show_text_101_mv_face_ref() {
    // 101 with 4 params (MV, no speakerName 5th): params[0] is the face name.
    let ir = load_mv();
    assert!(
        ir.asset_refs
            .iter()
            .any(|(k, _)| k.kind == AssetKind::Face && k.name == "Hero"),
        "101 face reference (Hero) is emitted on MV"
    );
}

#[test]
fn plugin_command_356_mv_is_a_call() {
    // 356 (MV): raw string "MyCommand arg1 arg2" → command = first token,
    // structured = false (MV does not separate plugin name from command).
    let ir = load_mv();
    assert!(
        ir.plugin_command_calls
            .iter()
            .any(|(c, _)| c.command == "MyCommand" && !c.structured && c.plugin.is_none()),
        "356 produces a best-effort (unstructured) plugin command call"
    );
}

#[test]
fn mv_animation_format_emits_animation_asset() {
    // MV animation (frames present) references img/animations via animation1Name.
    let ir = load_mv();
    assert!(
        ir.asset_refs
            .iter()
            .any(|(k, _)| k.kind == AssetKind::Animation && k.name == "Heal1"),
        "MV animation1Name (Heal1) is emitted as an Animation asset"
    );
}

#[test]
fn pipeline_finds_broken_transfer_on_mv() {
    // 201 → map #99 (does not exist) must be flagged, proving the rule pipeline
    // runs end-to-end on an MV project.
    let ir = load_mv();
    let ctx = RuleCtx::new(&ir);
    let f = BrokenTransfer.run(&ctx);
    assert_eq!(f.len(), 1, "exactly one broken transfer");
    assert!(matches!(f[0].message, Msg::BrokenTransfer { map_id: 99 }));
}
