//! Command-line argument parsing (clap derive).
//!
//! Positional path to the project root, output format, minimum level, and
//! lists of enabled/disabled rules by their id.

use crate::config::FailOn;
use camino::Utf8PathBuf;
use clap::{Parser, ValueEnum};
use dk_doctor_core::{Lang, Severity};

/// CLI arguments for `dk-doctor`.
#[derive(Debug, Parser)]
#[command(
    name = "dk-doctor",
    about = "Статический анализатор проектов RPG Maker MV/MZ",
    version
)]
pub struct Args {
    /// RPG Maker project root (folder with `data/`; `www/` is also tried).
    #[arg(default_value = ".")]
    pub project: Utf8PathBuf,

    /// Report format: human-readable console or JSON.
    #[arg(long, value_enum, default_value_t = Format::Console)]
    pub format: Format,

    /// Report language (`ru`/`en`). By default determined from the OS locale
    /// (`ru*` → Russian, otherwise English).
    #[arg(long, value_enum)]
    pub lang: Option<LangArg>,

    /// Minimum severity level to output (findings below it are discarded).
    #[arg(long, value_enum)]
    pub min_severity: Option<SeverityArg>,

    /// Enable only these rules (by id, can be repeated). If set, the remaining
    /// rules are disabled.
    #[arg(long = "enable", value_name = "RULE-ID")]
    pub enable: Vec<String>,

    /// Disable these rules (by id, can be repeated).
    #[arg(long = "disable", value_name = "RULE-ID")]
    pub disable: Vec<String>,

    /// Enable the `orphan-assets` rule (off by default: on stock RTP it produces
    /// hundreds of `info` lines, and without plugin parsing the list is incomplete).
    #[arg(long)]
    pub orphans: bool,

    /// Enable the `dead-common-event` rule (off by default: plugins often
    /// reserve common events via `$gameTemp.reserveCommonEvent`, which is not
    /// tracked in this iteration → on plugin-heavy projects almost all findings
    /// are false positives).
    #[arg(long = "dead-common-events")]
    pub dead_common_events: bool,

    /// Enable the `circular-gate` rule (off by default, prototype): finds
    /// progression deadlocks — clusters of global switches that mutually gate each
    /// other, so none can ever be turned on. Switches turned on by a plugin
    /// command are not tracked, so on plugin-heavy projects a "deadlock" may be a
    /// false positive.
    #[arg(long = "circular-gates")]
    pub circular_gates: bool,

    /// Enable the `blocked-tile` rule (off by default): checks tile passability of
    /// fixed destinations — a Transfer Player or the player's start landing on a
    /// tile impassable from all four directions. Passability plugins (region
    /// passage, pixel movement) are not accounted for, so a "blocked" tile may be
    /// a false positive.
    #[arg(long = "tiles")]
    pub tiles: bool,

    /// Enable the `db-reachability` rule (off by default): finds database records
    /// (enemies/skills/weapons/armors) referenced nowhere in the data. Plugin and
    /// notetag references are not tracked, so an "unused" record may be a false
    /// positive.
    #[arg(long = "db-reachability")]
    pub db_reachability: bool,

    /// Enable the `picture-lifecycle` rule (off by default): flags a picture
    /// operated on (Move/Rotate/Tint/Erase) before it is Shown in the same command
    /// list. Pictures persist across events, so a picture shown by another
    /// event/script cannot be seen statically → an op may be a false positive.
    #[arg(long = "pictures")]
    pub pictures: bool,

    /// Path to the project config (`.dk-doctor.toml`). Defaults to
    /// `<project>/.dk-doctor.toml`; if absent, built-in defaults are used.
    #[arg(long, value_name = "PATH")]
    pub config: Option<Utf8PathBuf>,

    /// CI gate: which findings make the process exit non-zero. Overrides the
    /// config. Without it, the legacy exit code is used (2 errors / 1 warnings / 0).
    #[arg(long = "fail-on", value_enum)]
    pub fail_on: Option<FailOnArg>,

    /// Baseline file of finding fingerprints (JSON array) used by
    /// `--fail-on new`. Defaults to the config `baseline` if set.
    #[arg(long, value_name = "PATH")]
    pub baseline: Option<Utf8PathBuf>,

    /// Write the current findings' fingerprints to this file (a fresh baseline)
    /// and exit without gating.
    #[arg(long = "write-baseline", value_name = "PATH")]
    pub write_baseline: Option<Utf8PathBuf>,
}

/// CI gate mode as a CLI argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FailOnArg {
    /// Never fail.
    Never,
    /// Fail on any error.
    Error,
    /// Fail on any warning or error.
    Warning,
    /// Fail on any finding.
    All,
    /// Fail only on findings absent from the baseline.
    New,
}

impl From<FailOnArg> for FailOn {
    fn from(value: FailOnArg) -> Self {
        match value {
            FailOnArg::Never => FailOn::Never,
            FailOnArg::Error => FailOn::Error,
            FailOnArg::Warning => FailOn::Warning,
            FailOnArg::All => FailOn::All,
            FailOnArg::New => FailOn::New,
        }
    }
}

/// Report output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    /// Human-readable report to the console.
    Console,
    /// JSON artifact.
    Json,
}

/// Report language as a CLI argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LangArg {
    /// Russian.
    Ru,
    /// English.
    En,
}

impl From<LangArg> for Lang {
    fn from(value: LangArg) -> Self {
        match value {
            LangArg::Ru => Lang::Ru,
            LangArg::En => Lang::En,
        }
    }
}

/// Resolves the language: explicit `--lang` → OS locale (sys-locale) → `LANG`/`LC_ALL`
/// → English. A locale starting with `ru` yields Russian, otherwise English.
pub fn resolve_lang(arg: Option<LangArg>) -> Lang {
    if let Some(a) = arg {
        return a.into();
    }
    let locale = sys_locale::get_locale()
        .or_else(|| std::env::var("LC_ALL").ok())
        .or_else(|| std::env::var("LANG").ok());
    match locale {
        Some(l) if l.to_ascii_lowercase().starts_with("ru") => Lang::Ru,
        _ => Lang::En,
    }
}

/// Severity level as a CLI argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SeverityArg {
    /// Info and above.
    Info,
    /// Warnings and above.
    Warning,
    /// Errors only.
    Error,
}

impl From<SeverityArg> for Severity {
    fn from(value: SeverityArg) -> Self {
        match value {
            SeverityArg::Info => Severity::Info,
            SeverityArg::Warning => Severity::Warning,
            SeverityArg::Error => Severity::Error,
        }
    }
}
