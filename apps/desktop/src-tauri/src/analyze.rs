//! The `analyze` command: replicates the CLI pipeline (`load_project` → rules → `Report`)
//! and returns the same language-neutral JSON as `dk-doctor --format json`.
//!
//! Embeds the analyzer AS A LIBRARY — without a subprocess/IPC. The rule registry,
//! command codes (EXIT/OPAQUE), severity filter, and sorting are copied from
//! `crates/cli/src/main.rs` so the artifact matches the CLI defaults.

use camino::Utf8PathBuf;
use dk_doctor_core::{Engine, Entity, Ir, Lang, Registry, Report, RuleCtx, Severity};
use dk_doctor_rpgmaker::{
    load_project, load_project_with_warnings, CommandLine, MapAtlas, MapRender,
};
use serde::{Deserialize, Serialize};

/// RPG Maker command code for "Exit Event Processing" (115) for `dead-code-after-exit`.
const EXIT_COMMAND_CODES: &[u16] = &[115];

/// "Untraceable exit" codes for `stuck-autorun`: common event call
/// (117), arbitrary script (355), MV/MZ plugin commands (356/357).
const OPAQUE_EXIT_CODES: &[u16] = &[117, 355, 356, 357];

/// Command code for the "Label" marker (118) for `dead-code-after-exit` (target of "Jump to
/// Label" 119). Must match the CLI so the JSON stays identical.
const LABEL_COMMAND_CODES: &[u16] = &[118];

/// "Empty command" code (0) — the block/list terminator the editor appends. Passed to
/// `dead-code-after-exit` so a trailing terminator after an exit is not marked dead.
/// Must match the CLI so the JSON stays identical.
const NOOP_COMMAND_CODES: &[u16] = &[0];

/// Analysis options from the UI (mirror of the CLI flags).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AnalyzeOpts {
    /// Enable only these rules (by id). Non-empty ⇒ the rest are off.
    pub enable: Vec<String>,
    /// Disable these rules (by id).
    pub disable: Vec<String>,
    /// Enable the opt-in `orphan-assets`.
    pub orphans: bool,
    /// Enable the opt-in `dead-common-event`.
    pub dead_common_events: bool,
    /// Minimum severity: `"info"` | `"warning"` | `"error"` (otherwise no filter).
    pub min_severity: Option<String>,
}

/// Summary project statistics for the dashboard's left rail (context, not findings).
/// Computed from the public [`Ir`] API — the crates are not modified.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStats {
    /// Project engine (`"mv"`/`"mz"`).
    pub engine: &'static str,
    /// Number of maps.
    pub maps: usize,
    /// Number of events on maps.
    pub events: usize,
    /// Total number of commands: map event pages + common events.
    pub commands: u64,
    /// Number of enabled plugins (load order).
    pub plugins: usize,
    /// Number of assets actually present on disk.
    pub assets: usize,
}

/// Result of the [`scan`] command: project statistics + JSON report.
///
/// `report` is the same string returned by [`analyze`]/`dk-doctor --format json`
/// (the contract is preserved); the frontend parses it via `JSON.parse`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    /// Summary project statistics.
    pub stats: ProjectStats,
    /// JSON report (identical to CLI `--format json`).
    pub report: String,
    /// Project files that could not be parsed (skipped) — one message per file.
    /// The UI shows these so a partial report does not look complete when some
    /// data is unreadable.
    pub warnings: Vec<String>,
}

/// Scans an RPG Maker project at the path: loads it ONCE, computes
/// statistics, and runs the rules, returning a [`ScanResult`]. The primary command
/// for the UI (unlike [`analyze`], which is needed by the contract test).
///
/// On load failure (no `data/`, encrypted, not RPG Maker) returns
/// `Err(String)` — the frontend shows it in the "not-analyzable" state.
#[tauri::command]
pub async fn scan(
    path: String,
    lang: Option<String>,
    opts: Option<AnalyzeOpts>,
) -> Result<ScanResult, String> {
    // We move the heavy parsing off the WebView main thread (otherwise the UI hangs on
    // large projects). All arguments are owned ⇒ no borrowing issues.
    tauri::async_runtime::spawn_blocking(move || scan_blocking(path, lang, opts))
        .await
        .map_err(|e| e.to_string())?
}

/// Synchronous body of [`scan`] (invoked from the blocking pool).
fn scan_blocking(
    path: String,
    lang: Option<String>,
    opts: Option<AnalyzeOpts>,
) -> Result<ScanResult, String> {
    let opts = opts.unwrap_or_default();
    let lang = resolve_lang(lang.as_deref());
    let root = Utf8PathBuf::from(path);

    let (ir, warnings) =
        load_project_with_warnings(&root).map_err(|e| render_load_error(&e, lang))?;
    let engine = engine_str(ir.engine);

    let stats = compute_stats(&ir, engine);
    let report = run_and_render(&ir, engine, lang, &opts);
    Ok(ScanResult {
        stats,
        report,
        warnings,
    })
}

/// Returns the render-only map geometry sidecar for the "Atlas" view: per-map
/// size + event tile coordinates. Independent of the findings report (no IR /
/// contract impact); the UI joins findings to events by their location path.
///
/// Best-effort: on load failure returns `Err(String)` and the UI silently
/// falls back to the flat list (the geometry is a presentation aid only).
#[tauri::command]
pub async fn map_atlas(path: String) -> Result<Vec<MapAtlas>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dk_doctor_rpgmaker::map_atlas(&Utf8PathBuf::from(path)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Returns the full tile render data for one map (Wave 2 real tiles): geometry +
/// the layered tile-id array + the tileset image slot names. Render-only and
/// on-demand; independent of the findings report.
#[tauri::command]
pub async fn map_render(path: String, map_id: u32) -> Result<MapRender, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dk_doctor_rpgmaker::map_render(&Utf8PathBuf::from(path), map_id).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Returns one event page's command list (for the finding "context" view: the
/// surrounding commands with the offending line highlighted). Render-only.
#[tauri::command]
pub async fn event_commands(
    path: String,
    map_id: u32,
    event_id: u32,
    page: u32,
) -> Result<Vec<CommandLine>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        dk_doctor_rpgmaker::event_page_commands(&Utf8PathBuf::from(path), map_id, event_id, page)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Reads a project image (e.g. `img/tilesets/World.png`) and returns its raw
/// bytes as a binary IPC response — the WebView turns it into an `ArrayBuffer`
/// for `createImageBitmap`, so no asset protocol or base64 is needed.
#[tauri::command]
pub async fn read_project_image(root: String, rel: String) -> Result<tauri::ipc::Response, String> {
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        dk_doctor_rpgmaker::read_project_image(&Utf8PathBuf::from(root), &rel)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Analyzes an RPG Maker project at the path and returns a JSON report (like CLI
/// `--format json`). `lang` is `"ru"`/`"en"`; `None`/anything else ⇒ by OS locale.
///
/// Kept for the contract test (`contract_matches_cli`): its string
/// result is compared byte-for-byte against the CLI. The UI uses [`scan`].
#[tauri::command]
pub async fn analyze(
    path: String,
    lang: Option<String>,
    opts: Option<AnalyzeOpts>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || analyze_blocking(path, lang, opts))
        .await
        .map_err(|e| e.to_string())?
}

/// Synchronous body of the analysis (invoked from the blocking pool). Replicates the CLI pipeline.
fn analyze_blocking(
    path: String,
    lang: Option<String>,
    opts: Option<AnalyzeOpts>,
) -> Result<String, String> {
    let opts = opts.unwrap_or_default();
    let lang = resolve_lang(lang.as_deref());
    let root = Utf8PathBuf::from(path);

    let ir = load_project(&root).map_err(|e| render_load_error(&e, lang))?;
    let engine = engine_str(ir.engine);
    Ok(run_and_render(&ir, engine, lang, &opts))
}

/// Running the rules + severity filter + building/rendering the report — the shared pipeline of
/// [`analyze`] and [`scan`], guaranteeing identical JSON.
fn run_and_render(ir: &Ir, engine: &str, lang: Lang, opts: &AnalyzeOpts) -> String {
    let registry = build_registry(opts);
    let ctx = RuleCtx::with_codes(
        ir,
        EXIT_COMMAND_CODES,
        OPAQUE_EXIT_CODES,
        LABEL_COMMAND_CODES,
    )
    .with_noop_codes(NOOP_COMMAND_CODES);
    let mut findings = registry.run_all(&ctx);

    if let Some(min) = opts.min_severity.as_deref().and_then(parse_severity) {
        findings.retain(|f| f.severity >= min);
    }

    let report = Report::new(findings);
    crate::report_json::render(&report, engine, lang)
}

/// Computes the summary project statistics from the public [`Ir`] API.
fn compute_stats(ir: &Ir, engine: &'static str) -> ProjectStats {
    let mut events = 0usize;
    let mut commands = 0u64;
    for node in &ir.entities {
        match &node.kind {
            Entity::Event(_) => events += 1,
            // Map-event pages and common events both carry command lists; common
            // events do not produce Page entities, so count them here too.
            Entity::Page(p) => commands += u64::from(p.command_count),
            Entity::CommonEvent(ce) => commands += u64::from(ce.command_count),
            _ => {}
        }
    }
    ProjectStats {
        engine,
        maps: ir.maps_by_id.len(),
        events,
        commands,
        plugins: ir.plugin_meta.load_order.len(),
        assets: ir.assets_present.len(),
    }
}

/// `Engine` → stable engine string (`"mv"`/`"mz"`).
fn engine_str(engine: Engine) -> &'static str {
    match engine {
        Engine::Mv => "mv",
        Engine::Mz => "mz",
    }
}

/// Renders a localized load-error message (for the `Err` branch):
/// the error kind selects the user-friendly text, the detail is the path/file.
fn render_load_error(error: &dk_doctor_rpgmaker::AdapterError, lang: Lang) -> String {
    let (kind, detail) = error.to_load_error();
    dk_doctor_core::render_chrome(&dk_doctor_core::Chrome::LoadError { kind, detail }, lang)
}

/// Rule registry with opt-in gating — a copy of the `cli::build_registry` logic.
///
/// `orphan-assets` and `dead-common-event` stay OFF by default (they are noisy on
/// stock RTP / plugin-heavy projects); they are enabled only by an explicit flag.
fn build_registry(opts: &AnalyzeOpts) -> Registry {
    let mut registry = Registry::empty();
    for rule in builtin_rules() {
        let id = rule.id();
        if opts.disable.iter().any(|d| d == id) {
            continue;
        }
        let active = if id == "orphan-assets" {
            opts.orphans || opts.enable.iter().any(|e| e == id)
        } else if id == "dead-common-event" {
            opts.dead_common_events || opts.enable.iter().any(|e| e == id)
        } else if opts.enable.is_empty() {
            true
        } else {
            opts.enable.iter().any(|e| e == id)
        };
        if active {
            registry.register(rule);
        }
    }
    registry
}

/// List of built-in rules (identical to `cli::builtin_rules`).
fn builtin_rules() -> Vec<Box<dyn dk_doctor_core::Rule>> {
    use dk_doctor_core::rules;
    vec![
        Box::new(rules::dead_variables::DeadVariables),
        Box::new(rules::uninit_symbols::UninitSymbols),
        Box::new(rules::broken_transfer::BrokenTransfer),
        Box::new(rules::impossible_condition::ImpossibleCondition),
        Box::new(rules::unreachable_maps::UnreachableMaps),
        Box::new(rules::referential_integrity::ReferentialIntegrity),
        Box::new(rules::broken_assets::BrokenAssets),
        Box::new(rules::orphan_assets::OrphanAssets),
        Box::new(rules::dead_code_after_exit::DeadCodeAfterExit),
        Box::new(rules::dead_self_switch::DeadSelfSwitch),
        Box::new(rules::unreachable_self_switch::UnreachableSelfSwitch),
        Box::new(rules::dead_common_event::DeadCommonEvent),
        Box::new(rules::cyclic_common_events::CyclicCommonEvents),
        Box::new(rules::shadowed_page::ShadowedPage),
        Box::new(rules::stuck_autorun::StuckAutorun),
        Box::new(rules::plugin_load_order::PluginLoadOrder),
        Box::new(rules::missing_base::MissingBase),
        Box::new(rules::unknown_plugin_command::UnknownPluginCommand),
        Box::new(rules::plugin_conflict::PluginConflict),
        Box::new(rules::vehicle_start_map::VehicleStartMap),
    ]
}

/// `"info"`/`"warning"`/`"error"` → [`Severity`] (otherwise `None` = no filter).
fn parse_severity(s: &str) -> Option<Severity> {
    match s.to_ascii_lowercase().as_str() {
        "info" => Some(Severity::Info),
        "warning" => Some(Severity::Warning),
        "error" => Some(Severity::Error),
        _ => None,
    }
}

/// Mirror of `cli::resolve_lang`: explicit argument → OS locale → `En`.
fn resolve_lang(arg: Option<&str>) -> Lang {
    match arg.map(|s| s.to_ascii_lowercase()) {
        Some(ref l) if l == "ru" => return Lang::Ru,
        Some(ref l) if l == "en" => return Lang::En,
        _ => {}
    }
    let locale = sys_locale::get_locale()
        .or_else(|| std::env::var("LC_ALL").ok())
        .or_else(|| std::env::var("LANG").ok());
    match locale {
        Some(l) if l.to_ascii_lowercase().starts_with("ru") => Lang::Ru,
        _ => Lang::En,
    }
}
