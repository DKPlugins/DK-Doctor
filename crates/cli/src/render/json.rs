//! JSON report renderer: the same artifact that will later go to the server.
//!
//! Serializes [`Report`] (findings + summary) plus the detected engine. Field
//! names are stable. The finding's message is emitted **twice**: as
//! language-neutral structural data (`message_key` + `args`) and as a ready
//! `message` string in the selected language.

use dk_doctor_core::{
    Fix, Lang, Msg, Remediation, Report, autofix, remediation, render as render_msg,
};
use serde::Serialize;

/// JSON report wrapper with stable field names.
#[derive(Serialize)]
struct JsonReport<'a> {
    /// Detected project engine (`mv`/`mz`).
    engine: &'a str,
    /// Language in which the `message` strings are rendered.
    lang: Lang,
    /// Summary by severity levels.
    summary: &'a dk_doctor_core::Summary,
    /// Sorted findings.
    findings: Vec<Finding<'a>>,
}

/// Finding in JSON form (stable names; includes engine-independent location).
#[derive(Serialize)]
struct Finding<'a> {
    rule: &'a str,
    severity: dk_doctor_core::Severity,
    category: dk_doctor_core::Category,
    confidence: dk_doctor_core::Confidence,
    /// Stable, language-neutral fingerprint (rule + file + path + args). Consumers
    /// (baselines, the desktop run-diff) key "the same finding" on this.
    fingerprint: String,
    file: &'a camino::Utf8Path,
    path: String,
    /// Stable message key (language-neutral), e.g. `"orphan_asset"`.
    message_key: &'static str,
    /// Typed message arguments (language-neutral).
    args: &'a Msg,
    /// Ready message string in the selected language.
    message: String,
    references: Vec<Reference<'a>>,
    /// Remediation metadata (why it matters, how to fix, docs link) — computed
    /// from the message; feeds the desktop bug-card / rule-explainer.
    remediation: Remediation,
    /// A safe, machine-applicable fix (only case-only asset renames today);
    /// omitted when the finding has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    fix: Option<Fix>,
}

/// Related site in JSON form.
#[derive(Serialize)]
struct Reference<'a> {
    file: &'a camino::Utf8Path,
    path: String,
}

/// Stable language-neutral key for the message variant.
fn message_key(msg: &Msg) -> &'static str {
    match msg {
        Msg::DeadVariable { .. } => "dead_variable",
        Msg::UninitializedSymbol { .. } => "uninitialized_symbol",
        Msg::BrokenTransfer { .. } => "broken_transfer",
        Msg::BrokenTransferVar { .. } => "broken_transfer_var",
        Msg::VehicleStartMapMissing { .. } => "vehicle_start_map_missing",
        Msg::UnreachableMap { .. } => "unreachable_map",
        Msg::DanglingDbRef { .. } => "dangling_db_ref",
        Msg::BrokenAsset { .. } => "broken_asset",
        Msg::AssetCaseMismatch { .. } => "asset_case_mismatch",
        Msg::OrphanAsset { .. } => "orphan_asset",
        Msg::DeadCodeAfterExit { .. } => "dead_code_after_exit",
        Msg::DeadSelfSwitch { .. } => "dead_self_switch",
        Msg::UnreachableSelfSwitch { .. } => "unreachable_self_switch",
        Msg::DeadCommonEvent { .. } => "dead_common_event",
        Msg::CyclicCommonEvents { .. } => "cyclic_common_events",
        Msg::ShadowedPage { .. } => "shadowed_page",
        Msg::StuckAutorun { .. } => "stuck_autorun",
        Msg::PluginLoadOrder { .. } => "plugin_load_order",
        Msg::MissingBase { .. } => "missing_base",
        Msg::UnknownPluginCommand { .. } => "unknown_plugin_command",
        Msg::PluginConflict { .. } => "plugin_conflict",
        Msg::ImpossibleCondition { .. } => "impossible_condition",
        Msg::CircularGate { .. } => "circular_gate",
        Msg::TransferToBlockedTile { .. } => "transfer_to_blocked_tile",
        Msg::StartInWall { .. } => "start_in_wall",
        Msg::PictureBeforeShow { .. } => "picture_before_show",
        Msg::EmptyAutorunPage { .. } => "empty_autorun_page",
        Msg::EmptyParallelPage { .. } => "empty_parallel_page",
        Msg::UnusedDbRecord { .. } => "unused_db_record",
    }
}

/// Serializes the report into a JSON string (indented) in the selected language.
pub fn render(report: &Report, engine: &str, lang: Lang) -> String {
    let findings: Vec<Finding> = report
        .findings
        .iter()
        .map(|f| Finding {
            rule: f.rule,
            severity: f.severity,
            category: f.category,
            confidence: f.confidence,
            fingerprint: f.fingerprint(),
            file: &f.location.file,
            path: f.location.path.to_string(),
            message_key: message_key(&f.message),
            args: &f.message,
            message: render_msg(&f.message, lang),
            references: f
                .references
                .iter()
                .map(|r| Reference {
                    file: &r.file,
                    path: r.path.to_string(),
                })
                .collect(),
            remediation: remediation(&f.message, lang),
            fix: autofix(&f.message),
        })
        .collect();

    let out = JsonReport {
        engine,
        lang,
        summary: &report.summary,
        findings,
    };
    serde_json::to_string_pretty(&out).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
}
