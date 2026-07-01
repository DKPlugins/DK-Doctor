//! Analyzer finding ([`Finding`]) and its taxonomy.
//!
//! Finding contract: severity, category, confidence, location, message,
//! related sites and a stable rule id. [`Severity`] is ordered so that
//! sorting puts errors first.

use crate::ir::location::Location;
use crate::message::Msg;

/// Analyzer finding: a single report entry.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Finding {
    /// Severity level.
    pub severity: Severity,
    /// Finding category.
    pub category: Category,
    /// Confidence.
    pub confidence: Confidence,
    /// Primary location of the finding.
    pub location: Location,
    /// Typed finding message (rendered by the catalog per language).
    pub message: Msg,
    /// Related sites (other reads/writes, etc.).
    pub references: Vec<Location>,
    /// Stable rule id, for example `"broken-transfer"`.
    pub rule: &'static str,
}

impl Finding {
    /// Language-neutral fingerprint identifying this finding across runs.
    ///
    /// Built from the same four components the desktop app hashes
    /// (`rule` + `file` + breadcrumb `path` + the structured message `args`), so a
    /// CLI baseline and the app's run-diff agree on what "the same finding" is.
    /// The localized `message` text is deliberately excluded — switching
    /// `--lang` must not make every finding look new. The engine output is
    /// deterministic, so the same underlying issue yields the same fingerprint on
    /// every run. Returned as a 16-char hex FNV-1a hash for compact baselines.
    pub fn fingerprint(&self) -> String {
        // `args` = the compact JSON of the typed message (tag "key" + fields),
        // matching the desktop's `JSON.stringify(f.args)`.
        let args = serde_json::to_string(&self.message).unwrap_or_default();
        let identity = format!(
            "{}{}{}{}",
            self.rule, self.location.file, self.location.path, args
        );
        format!("{:016x}", fnv1a64(identity.as_bytes()))
    }
}

/// FNV-1a 64-bit hash — a small, dependency-free, stable string hash for
/// finding fingerprints (baselines must stay comparable across builds, so we
/// avoid `DefaultHasher`, whose output is not guaranteed stable).
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Severity level of a finding.
///
/// `Ord` defines the order `Info < Warning < Error`, so sorting in
/// descending order puts errors first.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Info.
    Info,
    /// Warning.
    Warning,
    /// Error.
    Error,
}

/// Finding category.
///
/// `PluginOrder`/`PluginConflict` are reserved for the future plugin layer.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    /// Data (symbols, etc.).
    Data,
    /// Referential integrity.
    Reference,
    /// Assets.
    Asset,
    /// Dead code.
    DeadCode,
    /// Plugin load order (reserved).
    PluginOrder,
    /// Plugin conflict (reserved).
    PluginConflict,
}

/// Confidence of a finding.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// Static analysis over data — certain.
    Certain,
    /// Plugin AST heuristic — likely.
    Likely,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Location, PathSeg};

    fn finding(rule: &'static str, file: &str, id: u32) -> Finding {
        Finding {
            severity: Severity::Warning,
            category: Category::Data,
            confidence: Confidence::Certain,
            location: Location::new(file, vec![PathSeg::Map(3), PathSeg::Event(5)]),
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
    fn fingerprint_is_stable_and_16_hex() {
        let f = finding("dead-variables", "data/Map003.json", 7);
        let fp = f.fingerprint();
        assert_eq!(fp.len(), 16, "16-char hex");
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
        // Deterministic: recomputing yields the same value.
        assert_eq!(fp, f.clone().fingerprint());
    }

    #[test]
    fn fingerprint_distinguishes_rule_location_and_args() {
        let base = finding("dead-variables", "data/Map003.json", 7);
        let other_rule = finding("dead-self-switch", "data/Map003.json", 7);
        let other_file = finding("dead-variables", "data/Map004.json", 7);
        let other_args = finding("dead-variables", "data/Map003.json", 8);
        assert_ne!(base.fingerprint(), other_rule.fingerprint());
        assert_ne!(base.fingerprint(), other_file.fingerprint());
        assert_ne!(base.fingerprint(), other_args.fingerprint());
    }

    #[test]
    fn fingerprint_ignores_localized_message() {
        // Two findings that are identical except confidence/severity metadata
        // (which are NOT part of the identity) must share a fingerprint — the
        // fingerprint tracks the underlying issue, not presentation.
        let mut a = finding("dead-variables", "data/Map003.json", 7);
        let b = finding("dead-variables", "data/Map003.json", 7);
        a.severity = Severity::Error;
        a.confidence = Confidence::Likely;
        assert_eq!(a.fingerprint(), b.fingerprint());
    }
}
