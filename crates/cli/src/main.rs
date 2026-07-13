//! `dk-doctor` — CLI binary of the RPG Maker MV/MZ project analyzer.
//!
//! Flow: argument parsing → config (`.dk-doctor.toml`) merge → [`load_project`] →
//! running the rules [`Registry`] → suppressions → assembling the [`Report`] →
//! rendering (console/JSON) → exit code. The exit code is either the legacy
//! severity mapping (`0` clean / `1` warnings / `2` errors) or, with `--fail-on`,
//! a CI gate (`1` when the gate trips, else `0`).

mod args;
mod config;
mod render;

use args::{Args, Format, resolve_lang};
use clap::Parser;
use config::{FailOn, FileConfig};
use dk_doctor_core::{Chrome, Engine, Registry, Report, RuleCtx, Severity, render_chrome};
use dk_doctor_rpgmaker::{LoadOptions, load_project_with_warnings_options};
use std::collections::HashSet;
use std::process::ExitCode;

/// RPG Maker "Exit Event Processing" command code (115). Passed to the
/// `dead-code-after-exit` rule via [`RuleCtx::with_codes`]; the core itself does
/// not know the semantics of the code.
const EXIT_COMMAND_CODES: &[u16] = &[115];

/// Codes of "untraceable exit" commands for `stuck-autorun`: an arbitrary script
/// (355) and MV/MZ plugin commands (356/357). A common event call (117) is
/// handled interprocedurally by the rule via the per-common-event summary.
const OPAQUE_EXIT_CODES: &[u16] = &[355, 356, 357];

/// RPG Maker "Label" command code (118) — the target of "Jump to Label" (119).
/// Passed to `dead-code-after-exit` so that code after a label is not marked dead
/// (a jump may bypass the event exit).
const LABEL_COMMAND_CODES: &[u16] = &[118];

/// RPG Maker "empty command" code (0) — the block/list terminator the editor
/// appends at the end of every command list and indent block. Passed to
/// `dead-code-after-exit` so a trailing terminator after an exit is not marked dead.
const NOOP_COMMAND_CODES: &[u16] = &[0];

/// Effective settings after merging the CLI flags over the config file.
struct Effective {
    enable: Vec<String>,
    disable: Vec<String>,
    orphans: bool,
    dead_common_events: bool,
    circular_gates: bool,
    tiles: bool,
    db_reachability: bool,
    pictures: bool,
    min_severity: Option<Severity>,
    fail_on: Option<FailOn>,
    suppressed: HashSet<String>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let lang = resolve_lang(args.lang);

    // Load the project config. With --no-project-config the project's own
    // `.dk-doctor.toml` is treated as attacker-controlled and ignored (it can
    // otherwise set `fail_on = "never"`, disable rules, or suppress findings to
    // bypass the CI gate). An explicit operator-supplied `--config PATH` is
    // always honored.
    let (cfg_path, explicit, auto_load_disabled) = match (&args.config, args.no_project_config) {
        (Some(p), _) => (p.clone(), true, false),
        (None, false) => (args.project.join(".dk-doctor.toml"), false, false),
        (None, true) => (args.project.join(".dk-doctor.toml"), false, true),
    };
    let cfg = if auto_load_disabled {
        FileConfig::default()
    } else {
        match FileConfig::load(&cfg_path) {
            Ok(Some(c)) => c,
            Ok(None) if explicit => {
                eprintln!("dk-doctor: config not found: {cfg_path}");
                return ExitCode::from(2);
            }
            Ok(None) => FileConfig::default(),
            Err(e) => {
                eprintln!("dk-doctor: config error: {e}");
                return ExitCode::from(2);
            }
        }
    };

    let eff = match resolve_effective(&args, &cfg) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("dk-doctor: {e}");
            return ExitCode::from(2);
        }
    };

    // Validate --enable/--disable ids (merged with the config) against the real
    // rule set. A typo would otherwise silently match no rule.
    let known: Vec<&'static str> = builtin_rules().iter().map(|r| r.id()).collect();
    for id in eff.enable.iter().chain(eff.disable.iter()) {
        if !known.contains(&id.as_str()) {
            eprintln!(
                "dk-doctor: unknown rule id '{id}'. Known rules: {}",
                known.join(", ")
            );
            return ExitCode::from(2);
        }
    }

    let load_opts = if args.no_project_config {
        LoadOptions::untrusted()
    } else {
        LoadOptions::default()
    };
    let (ir, load_warnings) = match load_project_with_warnings_options(&args.project, &load_opts) {
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

    // Rule registry accounting for enable/disable and opt-in rules.
    let registry = build_registry(&eff);
    let orphans_active = registry.rule_ids().any(|id| id == "orphan-assets");
    let dead_ce_active = registry.rule_ids().any(|id| id == "dead-common-event");
    let circular_gates_active = registry.rule_ids().any(|id| id == "circular-gate");
    let tiles_active = registry.rule_ids().any(|id| id == "blocked-tile");
    let db_reachability_active = registry.rule_ids().any(|id| id == "db-reachability");
    let pictures_active = registry.rule_ids().any(|id| id == "picture-lifecycle");

    let ctx = RuleCtx::with_codes(
        &ir,
        EXIT_COMMAND_CODES,
        OPAQUE_EXIT_CODES,
        LABEL_COMMAND_CODES,
    )
    .with_noop_codes(NOOP_COMMAND_CODES);
    let findings = registry.run_all(&ctx);

    // Suppressions ([[suppress]] in the config) drop acknowledged findings before
    // anything else — they must not count toward the summary, the gate or output.
    let (findings, suppressed_count) = config::apply_suppressions(findings, &eff.suppressed);

    // --write-baseline: snapshot the current fingerprints and exit (no gating).
    if let Some(path) = &args.write_baseline {
        match config::write_baseline(path, &findings) {
            Ok(count) => {
                eprintln!(
                    "dk-doctor: {}",
                    render_chrome(&Chrome::BaselineWritten { count }, lang)
                );
                return ExitCode::SUCCESS;
            }
            Err(e) => {
                eprintln!("dk-doctor: {e}");
                return ExitCode::from(2);
            }
        }
    }

    // Full report (true summary) — the display filter below never masks it.
    let report = Report::new(findings);

    // Exit code: the CI gate if --fail-on/config set it, otherwise the legacy
    // severity mapping (2 errors / 1 warnings / 0).
    let baseline_set = if eff.fail_on == Some(FailOn::New) {
        match resolve_baseline(&args, &cfg) {
            Ok(set) => set,
            Err(e) => {
                eprintln!("dk-doctor: {e}");
                return ExitCode::from(2);
            }
        }
    } else {
        HashSet::new()
    };
    let (exit, new_count) = match eff.fail_on {
        Some(mode) => {
            let tripped = mode.triggered(&report, &baseline_set);
            let new = (mode == FailOn::New).then(|| mode.offending_count(&report, &baseline_set));
            (if tripped { 1u8 } else { 0u8 }, new)
        }
        None => (report.exit_code() as u8, None),
    };

    // Display filter: drop findings below the requested minimum from the output
    // only. Applied to a clone so the report above keeps the true summary.
    let mut shown = report.findings.clone();
    if let Some(min) = eff.min_severity {
        shown.retain(|f| f.severity >= min);
    }
    let shown_report = Report::new(shown);

    match args.format {
        Format::Console => {
            render::console::render_report(&shown_report, engine, lang);
            if !orphans_active {
                render::console::print_orphans_hint(lang);
            }
            if !dead_ce_active {
                render::console::print_dead_common_events_hint(lang);
            }
            if !circular_gates_active {
                render::console::print_circular_gates_hint(lang);
            }
            if !tiles_active {
                render::console::print_tiles_hint(lang);
            }
            if !db_reachability_active {
                render::console::print_db_reachability_hint(lang);
            }
            if !pictures_active {
                render::console::print_pictures_hint(lang);
            }
            if suppressed_count > 0 {
                eprintln!(
                    "{}",
                    render_chrome(
                        &Chrome::SuppressedNote {
                            count: suppressed_count
                        },
                        lang
                    )
                );
            }
            if let Some(count) = new_count {
                eprintln!(
                    "{}",
                    render_chrome(&Chrome::NewFindingsNote { count }, lang)
                );
            }
            // Surface skipped (unparseable) files so the report isn't silently partial.
            render::console::print_parse_warnings(&load_warnings, lang);
        }
        Format::Json => {
            use std::io::Write;
            let json = render::json::render(&shown_report, engine, lang);
            // Print JSON directly to stdout without coloring.
            let _ = writeln!(std::io::stdout(), "{json}");
        }
    }

    ExitCode::from(exit)
}

/// Merges the CLI flags over the config file into the effective settings. CLI
/// flags always win; a config value fills a gap the flag left. Invalid config
/// strings (`min_severity`/`fail_on`) are a hard error.
fn resolve_effective(args: &Args, cfg: &FileConfig) -> Result<Effective, String> {
    // enable/disable: the CLI list wins if non-empty, else the config list.
    let take = |cli: &[String], file: &[String]| -> Vec<String> {
        if cli.is_empty() {
            file.to_vec()
        } else {
            cli.to_vec()
        }
    };
    let min_severity = match args.min_severity {
        Some(s) => Some(Severity::from(s)),
        None => match &cfg.min_severity {
            Some(s) => Some(
                config::parse_min_severity(s)
                    .ok_or_else(|| format!("config: invalid min_severity '{s}'"))?,
            ),
            None => None,
        },
    };
    let fail_on = match args.fail_on {
        Some(a) => Some(FailOn::from(a)),
        None => match &cfg.fail_on {
            Some(s) => {
                Some(FailOn::parse(s).ok_or_else(|| format!("config: invalid fail_on '{s}'"))?)
            }
            None => None,
        },
    };
    Ok(Effective {
        enable: take(&args.enable, &cfg.enable),
        disable: take(&args.disable, &cfg.disable),
        orphans: args.orphans || cfg.orphans,
        dead_common_events: args.dead_common_events || cfg.dead_common_events,
        circular_gates: args.circular_gates || cfg.circular_gates,
        tiles: args.tiles || cfg.tiles,
        db_reachability: args.db_reachability || cfg.db_reachability,
        pictures: args.pictures || cfg.pictures,
        min_severity,
        fail_on,
        suppressed: cfg.suppressed_fingerprints(),
    })
}

/// Resolves the baseline set for `--fail-on new`: `--baseline`, else the config
/// `baseline` (relative to the project root), else empty.
fn resolve_baseline(args: &Args, cfg: &FileConfig) -> Result<HashSet<String>, String> {
    let path = args
        .baseline
        .clone()
        .or_else(|| cfg.baseline.as_ref().map(|b| args.project.join(b)));
    match path {
        Some(p) => config::read_baseline(&p),
        None => Ok(HashSet::new()),
    }
}

/// Assembles the rule registry, applying enable/disable by id.
///
/// If `enable` is non-empty, only the listed rules remain; otherwise all built-in
/// rules are taken, then `disable` is subtracted. `orphan-assets`/`dead-common-event`
/// stay opt-in unless enabled via their flag/config or an explicit `enable`.
fn build_registry(eff: &Effective) -> Registry {
    let mut registry = Registry::empty();
    for rule in builtin_rules() {
        let id = rule.id();
        // disable always wins.
        if eff.disable.iter().any(|d| d == id) {
            continue;
        }
        let active = if id == "orphan-assets" {
            eff.orphans || eff.enable.iter().any(|e| e == id)
        } else if id == "dead-common-event" {
            eff.dead_common_events || eff.enable.iter().any(|e| e == id)
        } else if id == "circular-gate" {
            eff.circular_gates || eff.enable.iter().any(|e| e == id)
        } else if id == "blocked-tile" {
            eff.tiles || eff.enable.iter().any(|e| e == id)
        } else if id == "db-reachability" {
            eff.db_reachability || eff.enable.iter().any(|e| e == id)
        } else if id == "picture-lifecycle" {
            eff.pictures || eff.enable.iter().any(|e| e == id)
        } else if eff.enable.is_empty() {
            true
        } else {
            eff.enable.iter().any(|e| e == id)
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
        Box::new(rules::circular_gate::CircularGate),
        Box::new(rules::picture_lifecycle::PictureLifecycle),
        Box::new(rules::empty_event_page::EmptyEventPage),
        Box::new(rules::blocked_tile::BlockedTile),
        Box::new(rules::db_reachability::DbReachability),
    ]
}

/// Engine label for the report.
fn engine_label(engine: Engine) -> &'static str {
    match engine {
        Engine::Mv => "mv",
        Engine::Mz => "mz",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn effective(args: &[&str]) -> Effective {
        let mut full = vec!["dk-doctor"];
        full.extend_from_slice(args);
        let parsed = Args::parse_from(full);
        resolve_effective(&parsed, &FileConfig::default()).unwrap()
    }

    fn ids(args: &[&str]) -> Vec<String> {
        build_registry(&effective(args))
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
        // The progression-deadlock prototype is off by default too.
        assert!(!active.iter().any(|id| id == "circular-gate"));
        // Spatial + DB-reachability + picture-lifecycle prototypes are off by
        // default (all FP-prone `likely` rules, like the others above).
        assert!(!active.iter().any(|id| id == "blocked-tile"));
        assert!(!active.iter().any(|id| id == "db-reachability"));
        assert!(!active.iter().any(|id| id == "picture-lifecycle"));
        // Regular rules stay enabled.
        assert!(active.iter().any(|id| id == "referential-integrity"));
        assert!(active.iter().any(|id| id == "cyclic-common-events"));
        assert!(active.iter().any(|id| id == "dead-self-switch"));
        // empty-event-page is the one new default-on rule (low false-positive rate).
        assert!(active.iter().any(|id| id == "empty-event-page"));
    }

    #[test]
    fn tiles_db_reachability_and_pictures_flags_enable_their_rules() {
        let with_tiles = ids(&[".", "--tiles"]);
        assert!(with_tiles.iter().any(|id| id == "blocked-tile"));
        assert!(!with_tiles.iter().any(|id| id == "db-reachability"));
        assert!(!with_tiles.iter().any(|id| id == "picture-lifecycle"));

        let with_db = ids(&[".", "--db-reachability"]);
        assert!(with_db.iter().any(|id| id == "db-reachability"));
        assert!(!with_db.iter().any(|id| id == "blocked-tile"));

        let with_pics = ids(&[".", "--pictures"]);
        assert!(with_pics.iter().any(|id| id == "picture-lifecycle"));
        assert!(!with_pics.iter().any(|id| id == "blocked-tile"));
    }

    #[test]
    fn circular_gates_flag_enables_the_rule() {
        let active = ids(&[".", "--circular-gates"]);
        assert!(active.iter().any(|id| id == "circular-gate"));
        // Other opt-in rules are not turned on as a side effect.
        assert!(!active.iter().any(|id| id == "orphan-assets"));
        assert!(!active.iter().any(|id| id == "dead-common-event"));
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

    #[test]
    fn config_enables_opt_in_and_sets_fail_on() {
        // A config that turns on orphans + dead-common-events and sets fail_on.
        let cfg: FileConfig =
            toml::from_str("orphans = true\ndead_common_events = true\nfail_on = \"warning\"\n")
                .unwrap();
        let parsed = Args::parse_from(["dk-doctor", "."]);
        let eff = resolve_effective(&parsed, &cfg).unwrap();
        assert!(eff.orphans);
        assert!(eff.dead_common_events);
        assert_eq!(eff.fail_on, Some(FailOn::Warning));
        let active: Vec<String> = build_registry(&eff).rule_ids().map(String::from).collect();
        assert!(active.iter().any(|id| id == "orphan-assets"));
        assert!(active.iter().any(|id| id == "dead-common-event"));
    }

    #[test]
    fn cli_flag_overrides_config_fail_on() {
        let cfg: FileConfig = toml::from_str("fail_on = \"warning\"\n").unwrap();
        let parsed = Args::parse_from(["dk-doctor", ".", "--fail-on", "error"]);
        let eff = resolve_effective(&parsed, &cfg).unwrap();
        assert_eq!(eff.fail_on, Some(FailOn::Error));
    }

    #[test]
    fn invalid_config_fail_on_is_an_error() {
        let cfg: FileConfig = toml::from_str("fail_on = \"sometimes\"\n").unwrap();
        let parsed = Args::parse_from(["dk-doctor", "."]);
        assert!(resolve_effective(&parsed, &cfg).is_err());
    }
}
