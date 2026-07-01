//! Project config (`.dk-doctor.toml`) + baseline handling for CI adoption.
//!
//! The config carries defaults for the analysis (rule enable/disable, minimum
//! severity, opt-in rules), a `fail_on` gate mode, an optional baseline path and
//! a list of `[[suppress]]` entries (a fingerprint + a reason). CLI flags always
//! override the config. The baseline is a plain JSON array of finding
//! fingerprints — the same identity the desktop run-diff uses — so `--fail-on new`
//! only trips CI on findings introduced since the baseline was written.

use camino::Utf8Path;
use dk_doctor_core::{Finding, Report, Severity};
use std::collections::HashSet;

/// CI gate mode: what set of findings should make the process exit non-zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailOn {
    /// Never fail (exit 0 regardless of findings).
    Never,
    /// Fail only if there is at least one error.
    Error,
    /// Fail if there is at least one warning or error.
    Warning,
    /// Fail if there is any finding at all.
    All,
    /// Fail only if there are findings not present in the baseline.
    New,
}

impl FailOn {
    /// Parses the config/CLI string form (`never`/`error`/`warning`/`all`/`new`).
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.trim().to_ascii_lowercase().as_str() {
            "never" => FailOn::Never,
            "error" => FailOn::Error,
            "warning" => FailOn::Warning,
            "all" => FailOn::All,
            "new" => FailOn::New,
            _ => return None,
        })
    }

    /// Whether the gate trips for this report. `baseline` is the set of known
    /// fingerprints (only consulted for [`FailOn::New`]).
    pub fn triggered(self, report: &Report, baseline: &HashSet<String>) -> bool {
        match self {
            FailOn::Never => false,
            FailOn::Error => report.summary.errors > 0,
            FailOn::Warning => report.summary.errors > 0 || report.summary.warnings > 0,
            FailOn::All => !report.findings.is_empty(),
            FailOn::New => report
                .findings
                .iter()
                .any(|f| !baseline.contains(&f.fingerprint())),
        }
    }

    /// Number of findings that would trip the gate (for the CI note). For
    /// [`FailOn::New`] this is the count of findings absent from the baseline.
    pub fn offending_count(self, report: &Report, baseline: &HashSet<String>) -> usize {
        match self {
            FailOn::Never => 0,
            FailOn::Error => report.summary.errors,
            FailOn::Warning => report.summary.errors + report.summary.warnings,
            FailOn::All => report.findings.len(),
            FailOn::New => report
                .findings
                .iter()
                .filter(|f| !baseline.contains(&f.fingerprint()))
                .count(),
        }
    }
}

/// A suppressed finding: its fingerprint plus a mandatory human reason (so a
/// suppression is a documented decision, not a silent mute).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Suppress {
    /// Finding fingerprint (as emitted in the JSON report).
    pub fingerprint: String,
    /// Why it is suppressed (documentation; not used by the analysis).
    #[allow(dead_code)]
    pub reason: Option<String>,
}

/// Parsed `.dk-doctor.toml`. Every field is optional; missing ones fall back to
/// the built-in defaults / CLI flags.
#[derive(Debug, Default, Clone, serde::Deserialize)]
#[serde(default)]
pub struct FileConfig {
    /// Minimum severity to display (`info`/`warning`/`error`).
    pub min_severity: Option<String>,
    /// CI gate mode (`never`/`error`/`warning`/`all`/`new`).
    pub fail_on: Option<String>,
    /// Rules to enable exclusively (by id).
    pub enable: Vec<String>,
    /// Rules to disable (by id).
    pub disable: Vec<String>,
    /// Enable the opt-in `orphan-assets` rule.
    pub orphans: bool,
    /// Enable the opt-in `dead-common-event` rule.
    pub dead_common_events: bool,
    /// Baseline file path (relative to the project root) for `fail_on = "new"`.
    pub baseline: Option<String>,
    /// Documented suppressions (TOML `[[suppress]]`).
    #[serde(rename = "suppress")]
    pub suppress: Vec<Suppress>,
}

impl FileConfig {
    /// Reads and parses the config at `path`. Returns `Ok(None)` if the file does
    /// not exist, `Err` on a read/parse error (so an explicit `--config` that is
    /// broken is surfaced rather than silently ignored).
    pub fn load(path: &Utf8Path) -> Result<Option<Self>, String> {
        let text = match std::fs::read_to_string(path.as_std_path()) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{path}: {e}")),
        };
        toml::from_str::<Self>(&text)
            .map(Some)
            .map_err(|e| format!("{path}: {e}"))
    }

    /// The set of suppressed fingerprints.
    pub fn suppressed_fingerprints(&self) -> HashSet<String> {
        self.suppress
            .iter()
            .map(|s| s.fingerprint.clone())
            .collect()
    }
}

/// Reads a baseline file (JSON array of fingerprint strings). A missing file
/// yields an empty set (nothing is baselined yet); a malformed one is an error.
pub fn read_baseline(path: &Utf8Path) -> Result<HashSet<String>, String> {
    let text = match std::fs::read_to_string(path.as_std_path()) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(e) => return Err(format!("{path}: {e}")),
    };
    serde_json::from_str::<Vec<String>>(&text)
        .map(|v| v.into_iter().collect())
        .map_err(|e| format!("{path}: {e}"))
}

/// Writes the baseline: the sorted, de-duplicated fingerprints of `findings`,
/// as a pretty JSON array.
pub fn write_baseline(path: &Utf8Path, findings: &[Finding]) -> Result<usize, String> {
    let mut fps: Vec<String> = findings.iter().map(|f| f.fingerprint()).collect();
    fps.sort_unstable();
    fps.dedup();
    let json = serde_json::to_string_pretty(&fps).map_err(|e| e.to_string())?;
    std::fs::write(path.as_std_path(), json).map_err(|e| format!("{path}: {e}"))?;
    Ok(fps.len())
}

/// Splits findings into (kept, suppressed-count) by fingerprint.
pub fn apply_suppressions(
    findings: Vec<Finding>,
    suppressed: &HashSet<String>,
) -> (Vec<Finding>, usize) {
    if suppressed.is_empty() {
        return (findings, 0);
    }
    let mut kept = Vec::with_capacity(findings.len());
    let mut removed = 0usize;
    for f in findings {
        if suppressed.contains(&f.fingerprint()) {
            removed += 1;
        } else {
            kept.push(f);
        }
    }
    (kept, removed)
}

/// Coarse severity of the display filter for the config's `min_severity` string.
pub fn parse_min_severity(s: &str) -> Option<Severity> {
    Some(match s.trim().to_ascii_lowercase().as_str() {
        "info" => Severity::Info,
        "warning" => Severity::Warning,
        "error" => Severity::Error,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dk_doctor_core::{Category, Confidence, Location, Msg, PathSeg};

    fn finding(rule: &'static str, sev: Severity, id: u32) -> Finding {
        Finding {
            severity: sev,
            category: Category::Data,
            confidence: Confidence::Certain,
            location: Location::new("data/Map001.json", vec![PathSeg::Map(1)]),
            message: Msg::DeadVariable {
                id,
                name: None,
                writes: 1,
            },
            references: Vec::new(),
            rule,
        }
    }

    #[test]
    fn fail_on_parses_all_modes() {
        assert_eq!(FailOn::parse("never"), Some(FailOn::Never));
        assert_eq!(FailOn::parse("Error"), Some(FailOn::Error));
        assert_eq!(FailOn::parse(" warning "), Some(FailOn::Warning));
        assert_eq!(FailOn::parse("all"), Some(FailOn::All));
        assert_eq!(FailOn::parse("new"), Some(FailOn::New));
        assert_eq!(FailOn::parse("bogus"), None);
    }

    #[test]
    fn gate_modes_trigger_as_expected() {
        let report = Report::new(vec![
            finding("dead-variables", Severity::Warning, 1),
            finding("broken-transfer", Severity::Error, 2),
        ]);
        let empty = HashSet::new();
        assert!(!FailOn::Never.triggered(&report, &empty));
        assert!(FailOn::Error.triggered(&report, &empty));
        assert!(FailOn::Warning.triggered(&report, &empty));
        assert!(FailOn::All.triggered(&report, &empty));

        // warning-only report: Error gate passes, Warning gate trips.
        let warns = Report::new(vec![finding("dead-variables", Severity::Warning, 1)]);
        assert!(!FailOn::Error.triggered(&warns, &empty));
        assert!(FailOn::Warning.triggered(&warns, &empty));
    }

    #[test]
    fn new_gate_uses_baseline() {
        let f1 = finding("dead-variables", Severity::Warning, 1);
        let f2 = finding("broken-transfer", Severity::Error, 2);
        let baseline: HashSet<String> = [f1.fingerprint()].into_iter().collect();
        let report = Report::new(vec![f1, f2.clone()]);
        // f2 is not in the baseline → new → gate trips.
        assert!(FailOn::New.triggered(&report, &baseline));
        assert_eq!(FailOn::New.offending_count(&report, &baseline), 1);
        // With everything baselined → no new findings.
        let full: HashSet<String> = report.findings.iter().map(|f| f.fingerprint()).collect();
        assert!(!FailOn::New.triggered(&report, &full));
    }

    #[test]
    fn suppressions_remove_by_fingerprint() {
        let f1 = finding("dead-variables", Severity::Warning, 1);
        let f2 = finding("broken-transfer", Severity::Error, 2);
        let suppressed: HashSet<String> = [f1.fingerprint()].into_iter().collect();
        let (kept, removed) = apply_suppressions(vec![f1, f2], &suppressed);
        assert_eq!(removed, 1);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].rule, "broken-transfer");
    }

    #[test]
    fn config_parses_full_toml() {
        let toml = r#"
min_severity = "warning"
fail_on = "new"
disable = ["orphan-assets"]
orphans = true
baseline = ".dk-doctor-baseline.json"

[[suppress]]
fingerprint = "deadbeefdeadbeef"
reason = "known false positive in vendor map"
"#;
        let cfg: FileConfig = toml::from_str(toml).unwrap();
        assert_eq!(cfg.min_severity.as_deref(), Some("warning"));
        assert_eq!(cfg.fail_on.as_deref(), Some("new"));
        assert_eq!(cfg.disable, vec!["orphan-assets".to_string()]);
        assert!(cfg.orphans);
        assert_eq!(cfg.baseline.as_deref(), Some(".dk-doctor-baseline.json"));
        assert_eq!(cfg.suppress.len(), 1);
        assert_eq!(cfg.suppress[0].fingerprint, "deadbeefdeadbeef");
        assert_eq!(
            cfg.suppress[0].reason.as_deref(),
            Some("known false positive in vendor map")
        );
        assert_eq!(cfg.suppressed_fingerprints().len(), 1);
    }
}
