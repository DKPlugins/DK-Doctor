//! Final report ([`Report`]) — serializable artifact for `--format json`
//! and computing the CLI exit code.

use crate::finding::{Finding, Severity};

/// Analyzer report: sorted findings + summary by severity level.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Report {
    /// Findings, sorted "errors first", then by location.
    pub findings: Vec<Finding>,
    /// Summary of finding counts by severity level.
    pub summary: Summary,
}

/// Summary of finding counts by severity level.
#[derive(Clone, Copy, Debug, Default, serde::Serialize)]
pub struct Summary {
    /// Number of errors.
    pub errors: usize,
    /// Number of warnings.
    pub warnings: usize,
    /// Number of informational findings.
    pub infos: usize,
}

impl Report {
    /// Builds a report from findings: sorts them (errors first, then by location)
    /// and computes the summary.
    pub fn new(mut findings: Vec<Finding>) -> Self {
        findings.sort_by(|a, b| {
            b.severity
                .cmp(&a.severity)
                .then_with(|| a.location.file.cmp(&b.location.file))
                .then_with(|| {
                    a.location
                        .path
                        .to_string()
                        .cmp(&b.location.path.to_string())
                })
                .then_with(|| a.rule.cmp(b.rule))
        });

        let mut summary = Summary::default();
        for f in &findings {
            match f.severity {
                Severity::Error => summary.errors += 1,
                Severity::Warning => summary.warnings += 1,
                Severity::Info => summary.infos += 1,
            }
        }

        Self { findings, summary }
    }

    /// Exit code for CI: `2` on errors, `1` on warnings, otherwise `0`.
    pub fn exit_code(&self) -> i32 {
        if self.summary.errors > 0 {
            2
        } else if self.summary.warnings > 0 {
            1
        } else {
            0
        }
    }
}
