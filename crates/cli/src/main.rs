//! `dk-doctor` — CLI binary of the RPG Maker MV/MZ project analyzer.
//!
//! Flow: argument parsing → [`load_project`] → running the rules
//! [`Registry`] → assembling the [`Report`] → rendering (console/JSON) → exit code
//! (`0` clean / `1` has warnings / `2` has errors).

mod args;
mod render;

use args::{Args, Format, resolve_lang};
use clap::Parser;
use dk_doctor_core::{Chrome, Engine, Registry, Report, RuleCtx, Severity, render_chrome};
use dk_doctor_rpgmaker::load_project_with_warnings;
use std::process::ExitCode;

/// RPG Maker "Exit Event Processing" command code (115). Passed to the
/// `dead-code-after-exit` rule via [`RuleCtx::with_codes`]; the core itself does
/// not know the semantics of the code.
const EXIT_COMMAND_CODES: &[u16] = &[115];

/// Codes of "untraceable exit" commands for `stuck-autorun`: common event
/// call (117), MV/MZ plugin commands (356/357) and an arbitrary script (355).
/// A page with such a command may exit through a mechanism outside the static analysis — we don't flag it.
const OPAQUE_EXIT_CODES: &[u16] = &[117, 355, 356, 357];

/// RPG Maker "Label" command code (118) — the target of "Jump to Label" (119).
/// Passed to `dead-code-after-exit` so that code after a label is not marked dead
/// (a jump may bypass the event exit).
const LABEL_COMMAND_CODES: &[u16] = &[118];

fn main() -> ExitCode {
    let args = Args::parse();
    let lang = resolve_lang(args.lang);

    // Validate --enable/--disable ids against the real rule set. A typo would
    // otherwise silently match no rule: with --enable it disables the whole
    // analysis and prints a misleading "No problems found" with exit 0.
    let known: Vec<&'static str> = builtin_rules().iter().map(|r| r.id()).collect();
    for id in args.enable.iter().chain(args.disable.iter()) {
        if !known.contains(&id.as_str()) {
            eprintln!(
                "dk-doctor: unknown rule id '{id}'. Known rules: {}",
                known.join(", ")
            );
            return ExitCode::from(2);
        }
    }

    let (ir, load_warnings) = match load_project_with_warnings(&args.project) {
        Ok(pair) => pair,
        Err(e) => {
            let (kind, detail) = e.to_load_error();
            eprintln!(
                "dk-doctor: {}",
                render_chrome(&Chrome::LoadError { kind, detail }, lang)
            );
            return ExitCode::from(2);
        }
    };

    let engine = engine_label(ir.engine);

    // Rule registry accounting for --enable/--disable and opt-in --orphans.
    let registry = build_registry(&args);
    let orphans_active = registry.rule_ids().any(|id| id == "orphan-assets");
    let dead_ce_active = registry.rule_ids().any(|id| id == "dead-common-event");

    let ctx = RuleCtx::with_codes(
        &ir,
        EXIT_COMMAND_CODES,
        OPAQUE_EXIT_CODES,
        LABEL_COMMAND_CODES,
    );
    let findings = registry.run_all(&ctx);

    // Exit code reflects the real project state (all findings), independent of
    // the display filter below — `--min-severity` must not mask warnings/errors
    // that actually exist (that would make CI silently pass).
    let report = Report::new(findings);
    let exit = exit_code(&report);

    // Display filter: drop findings below the requested minimum from the output
    // only. Applied to a clone so the report above keeps the true summary.
    let mut shown = report.findings.clone();
    if let Some(min) = args.min_severity.map(Severity::from) {
        shown.retain(|f| f.severity >= min);
    }
    let report = Report::new(shown);

    match args.format {
        Format::Console => {
            render::console::render_report(&report, engine, lang);
            if !orphans_active {
                render::console::print_orphans_hint(lang);
            }
            if !dead_ce_active {
                render::console::print_dead_common_events_hint(lang);
            }
            // Surface skipped (unparseable) files so the report isn't silently partial.
            render::console::print_parse_warnings(&load_warnings, lang);
        }
        Format::Json => {
            use std::io::Write;
            let json = render::json::render(&report, engine, lang);
            // Print JSON directly to stdout without coloring.
            let _ = writeln!(std::io::stdout(), "{json}");
        }
    }

    exit
}

/// Assembles the rule registry, applying `--enable`/`--disable` by id.
///
/// If `--enable` is given, only the listed rules remain; otherwise all built-in
/// rules are taken, then `--disable` is subtracted from them.
fn build_registry(args: &Args) -> Registry {
    let mut registry = Registry::empty();
    for rule in builtin_rules() {
        let id = rule.id();
        // --disable always wins.
        if args.disable.iter().any(|d| d == id) {
            continue;
        }
        let active = if id == "orphan-assets" {
            // opt-in: only with --orphans or an explicit --enable orphan-assets
            // (noisy on stock RTP, the list is incomplete without parsing plugins).
            args.orphans || args.enable.iter().any(|e| e == id)
        } else if id == "dead-common-event" {
            // opt-in: only with --dead-common-events or an explicit --enable
            // (plugins reserve CEs via $gameTemp.reserveCommonEvent — on
            // plugin-heavy projects almost all findings are false).
            args.dead_common_events || args.enable.iter().any(|e| e == id)
        } else if args.enable.is_empty() {
            true
        } else {
            args.enable.iter().any(|e| e == id)
        };
        if active {
            registry.register(rule);
        }
    }
    registry
}

/// List of all built-in rules as boxes (the source for filtering).
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

/// Engine label for the report.
fn engine_label(engine: Engine) -> &'static str {
    match engine {
        Engine::Mv => "mv",
        Engine::Mz => "mz",
    }
}

/// Exit code based on the report (extracted for consistency with the stdout/stderr paths).
fn exit_code(report: &Report) -> ExitCode {
    ExitCode::from(report.exit_code() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn ids(args: &[&str]) -> Vec<String> {
        let mut full = vec!["dk-doctor"];
        full.extend_from_slice(args);
        let parsed = Args::parse_from(full);
        build_registry(&parsed)
            .rule_ids()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn opt_in_rules_off_by_default() {
        let active = ids(&["."]);
        // Noisy info rules are off by default.
        assert!(!active.iter().any(|id| id == "orphan-assets"));
        assert!(!active.iter().any(|id| id == "dead-common-event"));
        // Regular rules stay enabled.
        assert!(active.iter().any(|id| id == "referential-integrity"));
        assert!(active.iter().any(|id| id == "cyclic-common-events"));
        assert!(active.iter().any(|id| id == "dead-self-switch"));
    }

    #[test]
    fn dead_common_events_flag_enables_the_rule() {
        let active = ids(&[".", "--dead-common-events"]);
        assert!(active.iter().any(|id| id == "dead-common-event"));
        // Other opt-in rules (orphan-assets) are not enabled in the process.
        assert!(!active.iter().any(|id| id == "orphan-assets"));
    }

    #[test]
    fn explicit_enable_overrides_opt_in_default() {
        let active = ids(&[".", "--enable", "dead-common-event"]);
        assert!(active.iter().any(|id| id == "dead-common-event"));
    }
}
