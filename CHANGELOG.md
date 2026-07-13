# Changelog

All notable changes to dk-doctor are documented here. The same entries are shown
in the desktop app under **Changelog** (`apps/desktop/src/changelog.ts`).

The project follows [Semantic Versioning](https://semver.org/). Versions below
`1.0.0` are public betas: rule output and the JSON report may still change.

## [0.2.0] — 2026-07-13

Plugin profiles, spatial & lifecycle rules, remediation hints, and a hardened analyzer core.

### Added

- Name-alias inference for untyped plugin parameters — switch/variable/common-event type inferred from the parameter name suffix (MV plugins without `@type`). (#1)
- Curated plugin-profile tables — symbol, database, common-event, asset and command facts for plugins the analyzer does not parse directly. (#2)
- Tier B JS extensions — literal asset loads, MV plugin-command hooks, `Imported`/runtime-risk flags; plus a `cargo xtask mine-plugin-profile` miner. (#3)
- Interprocedural common-event summaries — `dead-common-event` and `stuck-autorun` now look through common-event calls. (#6)
- CLI project config, baselines and CI gate — `.dk-doctor.toml`, `--fail-on`, `--baseline`, stable finding fingerprints, a GitHub Actions workflow. (#6)
- Symbolic variable ranges — `impossible-condition` now reasons over ranges; new `circular-gate` rule finds progression deadlocks (opt-in). (#7)
- Desktop D4–D10 — release-readiness checklist, HTML/PNG export, map-transition graph, run history (Time Machine), watch mode, trust badges. (#8)
- Four new rules: `blocked-tile`, `empty-event-page`, `picture-lifecycle`, `db-reachability`. (#9)
- Remediation metadata on every finding — why, how to fix, docs link; `asset_case_rename` auto-fix for case-mismatch. (#10)
- Share/feedback overlay — turn a report or finding into a compact, path-sanitized Markdown payload for a GitHub issue. (#12)

### Fixed

- Const-leak in script conditions/operands, page-scoped self-switch exit, and missing Change-Equipment references. (#4)
- Audit fixes — cyclic-common-events stack overflow, game-data Control-Variables references, desktop path-traversal and URL validation. (#11)
- Security hardening — DoS-resistant complexity and protection against project-driven finding suppression (`--no-project-config`). (#13)
- Welcome, drawer and feedback UI polish. (#14)

### Changed

- Pinned a host-neutral Rust toolchain for cross-platform CI; the desktop build stays on `stable` for macOS cross-targets. (#5)

## [0.1.1] — 2026-06-30

Maintenance: fewer false positives.

### Added

- Plugin self-switch idiom (`$gameSelfSwitches.setValue([this._mapId, this._eventId, 'X'])`) is now resolved, improving dead-/unreachable-self-switch accuracy.

### Fixed

- `\v[n]` text escapes and by-variable amounts/positions now count as variable reads — no more false dead-variable reports.
- `dead-code-after-exit` no longer flags the editor's empty terminator command.

## [0.1.0] — 2026-06-30

Initial public beta. Offline, deterministic static analyzer for RPG Maker MV/MZ — CLI + Tauri desktop app.

### Added

- First public release: eight deterministic rules over project data, a curated report, JSON output and a cross-platform desktop app.

[0.2.0]: https://github.com/DKPlugins/DK-Doctor/releases/tag/v0.2.0
[0.1.1]: https://github.com/DKPlugins/DK-Doctor/releases/tag/v0.1.1
[0.1.0]: https://github.com/DKPlugins/DK-Doctor/releases/tag/v0.1.0
