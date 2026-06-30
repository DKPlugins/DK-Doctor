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
