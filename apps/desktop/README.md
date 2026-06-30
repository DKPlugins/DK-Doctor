# dk-doctor desktop (Tauri v2)

Cross-platform desktop GUI for **dk-doctor**, the static analyzer for RPG Maker
MV/MZ projects. It **embeds the Rust analyzer as a library** (no subprocess, no
IPC) and renders the same findings the CLI produces — the report JSON is
**byte-identical** to `dk-doctor --format json` (guaranteed by the contract test
`src-tauri/tests/contract_matches_cli.rs`).

The UI is a high-fidelity health report, fully **offline** (inline SVG icons,
system-font fallbacks, a CSP that blocks external requests).

## What it does

- **Welcome**: open via folder picker (Tauri dialog plugin) **or drag a project
  folder onto the window**; a **recent projects** list (persisted) with health
  pills and relative time. MV/MZ auto-detected, `www/` handled.
- **Scanning**: animated progress (stages + terminal "well") while the embedded
  engine runs; ends with the real health/severity summary.
- **Report dashboard** (overview): a **health ring** (score + letter grade) +
  **category tiles** + a **faceted findings list** grouped by **severity / category
  / map** with **pattern aggregation** ("+N more"). Left rail: project stats +
  severity/category/confidence facets. Toolbar: live search, group-by, re-run,
  **export to JSON**.
- **Finding drawer**: per-finding drill-down (location breadcrumb, diagnosis,
  confidence note, related sites). `j/k`/`↑/↓` navigate, `Esc` closes, copy path,
  hide (ignore) a finding.
- **Settings** (modal): **theme** (System / Light / Dark — token swap), **density**,
  **language** (System / Русский / English), and opt-in analysis rules
  (`orphan-assets`, `dead-common-event`). Persisted in `localStorage`.
- Graceful states: welcome, scanning, not-analyzable (load error), zero-findings
  (clean bill of health), results.

Theme defaults to **system**; language defaults to the OS locale. Changing the
language re-runs the analysis so engine-localized messages switch. Analysis is
**fully deterministic and local** — your project data never leaves your machine;
the opt-in rules are OFF by default, matching the CLI. The app can optionally
check GitHub Releases for a newer version on launch (a Settings toggle, on by
default); this is the only network request and sends no project data.

## Tauri commands

- `scan(path, lang, opts)` — the UI command: loads the project **once**, returns
  `{ stats, report }` where `stats` (maps/events/commands/plugins/assets) feeds the
  rail and `report` is the JSON string identical to the CLI.
- `analyze(path, lang, opts)` — kept for the contract test; same report string.
  `scan`/`analyze` share one `run_and_render` so the report stays byte-identical.
- `write_text_file(path, contents)` — writes the export file the user picked via
  the save dialog.

> Drift guard: `src-tauri/src/{report_json.rs, analyze.rs}` mirror the CLI's
> `render/json.rs` + rule list. When a rule or `Msg` variant is added to the CLI,
> sync both here and re-run the contract test (an exhaustive `match` makes message
> drift a compile error; rule-list drift is **not** caught automatically).

### Health-score formula (transparent)

```
penalty = 6·errors + 2·warnings + 0.5·infos
score   = clamp(round(100 − penalty), 0, 100)
band    = score ≥ 90 healthy | ≥ 70 minor | ≥ 40 needs-attention | else critical
```

Defined in `src/health.ts` (one place to tune). Zero findings ⇒ score 100, healthy.

## Architecture / placement

- Standalone cargo project at `src-tauri/` with its **own empty `[workspace]`**
  table, so it is NOT part of the repo root Cargo workspace and uses its own build
  dir (`src-tauri/target`). It never contends with the root workspace.
- Depends on the analyzer crates by **path** only:
  `dk-doctor-core` and `dk-doctor-rpgmaker` (`../../../crates/...`).
- Frontend is **Vite + vanilla TypeScript** (no framework). The TS layer is a pure
  consumer of the one `analyze(path, lang, opts)` command; no analyzer logic in TS.

## Prerequisites

- **Node 22+** and **pnpm 10+** (`npm i -g pnpm`).
- **Rust stable (MSVC on Windows)**. cargo must be on PATH:
  ```powershell
  $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
  ```
- **Windows:** WebView2 runtime (preinstalled on Win 11) + MSVC Build Tools.
- **macOS:** Xcode Command Line Tools (`xcode-select --install`). Mac is built only in CI.

The Tauri CLI is provided by the `@tauri-apps/cli` dev-dependency and invoked via
`pnpm tauri ...` — `cargo-tauri` is **not** required.

## Run / build (Windows, local)

```powershell
cd apps/desktop
pnpm install                       # installs @tauri-apps/cli@^2 (Tauri CLI is a devDep)
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

pnpm tauri dev                     # dev with hot reload (Vite :1420 + native window)
pnpm tauri build                   # release -> src-tauri/target/release/bundle/nsis/*-setup.exe
```

Outputs of `pnpm tauri build`:

```
apps/desktop/src-tauri/target/release/dk-doctor.exe
apps/desktop/src-tauri/target/release/bundle/nsis/dk-doctor_0.1.0_x64-setup.exe
```

Fast sanity checks (no webview):

```powershell
# Compile the embed standalone (proves the analyzer links in):
& "$env:USERPROFILE\.cargo\bin\cargo.exe" build --manifest-path apps\desktop\src-tauri\Cargo.toml

# Contract test: desktop JSON == `dk-doctor --format json` (en + ru) on the fixture:
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test  --manifest-path apps\desktop\src-tauri\Cargo.toml --test contract_matches_cli

# Frontend type-check + bundle:
pnpm --dir apps\desktop build
```

## macOS build

Mac cannot be built on the Windows dev box; it is validated by CI on `macos-latest`
(`.github/workflows/desktop-build.yml`). To build locally on a Mac:

```bash
cd apps/desktop
pnpm install
export PATH="$HOME/.cargo/bin:$PATH"
pnpm tauri build -- --target universal-apple-darwin
```

**Signing / notarization is skipped for now** (ad-hoc). Unsigned macOS builds
trigger Gatekeeper; right-click → Open, or
`xattr -dr com.apple.quarantine dk-doctor.app`. To enable real signing later, add
`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_PASSWORD`,
`APPLE_TEAM_ID`, `KEYCHAIN_PASSWORD` repo secrets and the keychain-import step from
the Tauri macOS signing docs. Unsigned Windows NSIS installers similarly trigger
SmartScreen — acceptable for now, documented here.

## CI

Pushes/PRs touching `apps/desktop/**` or `crates/**` run a `windows-latest` +
`macos-latest` matrix (`.github/workflows/desktop-build.yml`) that installs
node/pnpm/rust, builds the frontend, runs `tauri build`, and uploads the bundles
as artifacts. macOS is **CI-only** (cannot be validated on the Windows dev box).

## Regenerating icons

Icons under `src-tauri/icons/` are generated from a source PNG:

```powershell
pnpm tauri icon path\to\source-512.png
```

## Layout

```
apps/desktop/
  package.json  vite.config.ts  tsconfig.json  index.html
  src/                 # frontend (vanilla TS)
    api.ts             # invoke wrappers (scan/export) + contract types + pickers
    store.ts           # settings + recent projects (localStorage)
    icons.ts           # inline SVG icon set (offline; no CDN)
    health.ts          # health score + letter grade + ring color
    group.ts           # filter / group (severity·category·map) / aggregation / totals
    i18n.ts            # ru/en chrome labels (NOT finding text) + relative time
    render.ts          # HTML for all views: welcome/scanning/report/drawer/settings
    main.ts            # state machine, events, Tauri, drag-drop, themes/lang, export
    styles.css         # design-system tokens (light+dark swap) + app styles
  src-tauri/
    Cargo.toml         # own empty [workspace]; path deps on the analyzer crates
    build.rs  tauri.conf.json   # frameless? no — native frame; visible:false + CSP
    capabilities/default.json   # core:default + window:show + event + dialog open/save
    icons/
    src/
      main.rs          # thin bin -> dk_doctor_desktop_lib::run()
      lib.rs           # tauri::Builder + dialog plugin + scan/analyze/write_text_file + window-show fallback
      analyze.rs       # #[tauri::command] scan + analyze + ProjectStats (mirrors cli pipeline)
      report_json.rs   # JSON render (mirror of cli/src/render/json.rs)
    tests/
      contract_matches_cli.rs   # desktop JSON == CLI JSON (en + ru)
```
