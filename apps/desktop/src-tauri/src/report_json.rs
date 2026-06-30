//! JSON render of the report — a BYTE-FOR-BYTE copy of `crates/cli/src/render/json.rs`.
//!
//! Goal: the desktop returns the same artifact as `dk-doctor --format json`.
//! Uses only public items of `dk_doctor_core` (`Lang`, `Msg`,
//! `Report`, `render`, `Severity`, `Category`, `Confidence`, `Summary`).
//!
//! ATTENTION (duplication): if `crates/cli/src/render/json.rs` changes
//! (a new `Msg` variant, a field rename), this copy must be synchronized.
//! The exhaustive `match` in [`message_key`] without a `_` arm guarantees that a new
//! `Msg` variant breaks compilation of this copy — an early warning of drift.
//! Future cleanup: extract the render into a shared crate `dk-doctor-report-json`.

use dk_doctor_core::{render as render_msg, Lang, Msg, Report};
use serde::Serialize;

/// JSON wrapper of the report with stable field names.
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

/// Finding in JSON form (stable names; includes the engine-independent location).
#[derive(Serialize)]
struct Finding<'a> {
    rule: &'a str,
    severity: dk_doctor_core::Severity,
    category: dk_doctor_core::Category,
    confidence: dk_doctor_core::Confidence,
    file: &'a camino::Utf8Path,
    path: String,
    /// Stable message key (language-neutral), for example `"orphan_asset"`.
    message_key: &'static str,
    /// Typed message arguments (language-neutral).
    args: &'a Msg,
    /// Ready message string in the selected language.
    message: String,
    references: Vec<Reference<'a>>,
}

/// Related site in JSON form.
#[derive(Serialize)]
struct Reference<'a> {
    file: &'a camino::Utf8Path,
    path: String,
}

/// Stable language-neutral key for a message variant.
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
