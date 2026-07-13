/**
 * Built-in changelog data (English-only by design — a community-neutral record,
 * same rationale as the share/feedback payload in PR #14). Surfaced verbatim in
 * the "Changelog" overlay; UI chrome around it (title, "current" badge) is
 * localized via `i18n`, the entry text is not.
 *
 * Newest release first. `current` marks the running build's version.
 */

export type ChangelogKind = "feat" | "fix" | "other";

export interface ChangelogItem {
  kind: ChangelogKind;
  text: string;
  /** Source pull request number (informational; not a link). */
  pr?: number;
}

export interface ChangelogRelease {
  version: string;
  /** ISO date (yyyy-mm-dd) of the release / version bump. */
  date: string;
  /** True for the build the user is currently running. */
  current?: boolean;
  /** Optional one-line summary shown under the version header. */
  summary?: string;
  items: ChangelogItem[];
}

export const CHANGELOG: ChangelogRelease[] = [
  {
    version: "0.2.0",
    date: "2026-07-13",
    current: true,
    summary: "Plugin profiles, spatial & lifecycle rules, remediation hints, and a hardened analyzer core.",
    items: [
      { kind: "feat", text: "Name-alias inference for untyped plugin parameters — switch/variable/common-event type inferred from the parameter name suffix (MV plugins without @type).", pr: 1 },
      { kind: "feat", text: "Curated plugin-profile tables — symbol, database, common-event, asset and command facts for plugins the analyzer does not parse directly.", pr: 2 },
      { kind: "feat", text: "Tier B JS extensions — literal asset loads, MV plugin-command hooks, Imported/runtime-risk flags; plus a `cargo xtask mine-plugin-profile` miner.", pr: 3 },
      { kind: "feat", text: "Interprocedural common-event summaries — dead-common-event and stuck-autorun now look through common-event calls.", pr: 6 },
      { kind: "feat", text: "CLI project config, baselines and CI gate — `.dk-doctor.toml`, `--fail-on`, `--baseline`, stable finding fingerprints, a GitHub Actions workflow.", pr: 6 },
      { kind: "feat", text: "Symbolic variable ranges — impossible-condition now reasons over ranges; new circular-gate rule finds progression deadlocks (opt-in).", pr: 7 },
      { kind: "feat", text: "Desktop D4–D10 — release-readiness checklist, HTML/PNG export, map-transition graph, run history (Time Machine), watch mode, trust badges.", pr: 8 },
      { kind: "feat", text: "Four new rules: blocked-tile, empty-event-page, picture-lifecycle, db-reachability.", pr: 9 },
      { kind: "feat", text: "Remediation metadata on every finding — why, how to fix, docs link; `asset_case_rename` auto-fix for case-mismatch.", pr: 10 },
      { kind: "feat", text: "Share/feedback overlay — turn a report or finding into a compact, path-sanitized Markdown payload for a GitHub issue.", pr: 12 },
      { kind: "fix", text: "Const-leak in script conditions/operands, page-scoped self-switch exit, and missing Change-Equipment references.", pr: 4 },
      { kind: "fix", text: "Audit fixes — cyclic-common-events stack overflow, game-data Control-Variables references, desktop path-traversal and URL validation.", pr: 11 },
      { kind: "fix", text: "Security hardening — DoS-resistant complexity and protection against project-driven finding suppression (`--no-project-config`).", pr: 13 },
      { kind: "fix", text: "Welcome, drawer and feedback UI polish.", pr: 14 },
      { kind: "other", text: "Pinned a host-neutral Rust toolchain for cross-platform CI; the desktop build stays on `stable` for macOS cross-targets.", pr: 5 },
    ],
  },
  {
    version: "0.1.1",
    date: "2026-06-30",
    summary: "Maintenance: fewer false positives.",
    items: [
      { kind: "feat", text: "Plugin self-switch idiom (`$gameSelfSwitches.setValue([this._mapId, this._eventId, 'X'])`) is now resolved, improving dead-/unreachable-self-switch accuracy." },
      { kind: "fix", text: "`\\v[n]` text escapes and by-variable amounts/positions now count as variable reads — no more false dead-variable reports." },
      { kind: "fix", text: "`dead-code-after-exit` no longer flags the editor's empty terminator command." },
    ],
  },
  {
    version: "0.1.0",
    date: "2026-06-30",
    summary: "Initial public beta. Offline, deterministic static analyzer for RPG Maker MV/MZ — CLI + Tauri desktop app.",
    items: [
      { kind: "feat", text: "First public release: eight deterministic rules over project data, a curated report, JSON output and a cross-platform desktop app." },
    ],
  },
];
