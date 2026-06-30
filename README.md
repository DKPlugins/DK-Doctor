# dk-doctor

> A static analyzer for **RPG Maker MV/MZ** projects — a *health report* that finds real bugs and risks,
> not statistics.

`dk-doctor` reads your project's `data/`, `img/`, `audio/` and `js/plugins`, builds a graph + symbol tables, and
reports **actionable findings**: broken teleports, dead and uninitialized switches/variables, dangling references,
broken and orphaned assets, unreachable maps, dead code. Each line of the report is either *"here's a bug + the
exact place"* or *"here's a risk + why"* — never vanilla statistics.

> **Status: beta.** The deterministic analyzer ships as a CLI and a cross-platform desktop app, with a curated set
> of diagnostic rules over project data **and** plugins. It runs entirely on your machine — nothing is uploaded.

## What it catches

```
error   broken-transfer        Map007/EV012/page1/cmd8
        Transfer Player → Map099, which does not exist → crash on trigger.

warning uninitialized-symbols  Switch #14
        Read by 3 page conditions (Map003/EV005, Map007/EV002) but never set by any
        event → those pages are unreachable. (not yet cross-checked against plugin @param)

info    orphan-assets          img/pictures/old_logo.png
        Present on disk but referenced nowhere → possibly unused.
```

Every finding carries a **confidence** level so the report stays honest:

| confidence | meaning |
|---|---|
| `certain` | proven by static analysis of the data |
| `likely`  | AST/heuristic inference (e.g. plugins) — may have false positives |

## Supported engines

RPG Maker **MV and MZ** (their `/data` formats are nearly identical; the adapter handles the differences — e.g. the
`356` vs `357`/`657` plugin-command split, the `101` speaker field).

## Build & run

Requires a [Rust toolchain](https://rustup.rs) (1.85+; tested on 1.96).

```sh
# build
cargo build --release

# analyze a project (point at the folder that contains data/, img/, audio/, js/)
cargo run -p dk-doctor -- "/path/to/your/RPGMaker/project"

# machine-readable output (JSON — for CI or other tooling)
cargo run -p dk-doctor -- --format json "/path/to/project"
```

Useful flags: `--format console|json`, `--lang ru|en` (report language; defaults to the OS locale — `ru*` → Russian,
otherwise English), `--min-severity info|warning|error`, `--enable <rule>` / `--disable <rule>`, `--orphans` (enable
the opt-in `orphan-assets` rule).

The console and JSON reports are multilingual. JSON additionally emits each finding's language-neutral
`message_key` + structured `args` alongside the rendered `message`, so downstream tooling can re-render in any
language.

Exit codes (CI-friendly): `0` clean · `1` warnings present · `2` errors present.

## Diagnostic rules

Data & control flow:

| rule | severity | confidence | what it finds |
|---|---|---|---|
| `broken-transfer` | error | certain | `Transfer Player` to a non-existent map |
| `referential-integrity` | error | certain | command references a missing item/skill/troop/actor/… id |
| `broken-assets` | error | certain | reference to an image/audio file not on disk |
| `vehicle-start-map` | warning | likely | a boat/ship/airship start map that doesn't exist |
| `dead-variables` | warning | certain | variable written but never read |
| `uninitialized-symbols` | warning | likely | switch/variable read but never set |
| `impossible-condition` | warning | likely | a branch whose variable is a constant → one side is dead code |
| `dead-code-after-exit` | warning | certain | commands after `Exit Event Processing` |
| `dead-self-switch` | warning | certain | self switch set but never checked |
| `unreachable-self-switch` | warning | likely | page needs a self switch that nothing sets |
| `cyclic-common-events` | warning | certain | infinite common-event call cycle (command 117) |
| `shadowed-page` | warning | likely | event page shadowed by a later, looser-condition page |
| `stuck-autorun` | warning | likely | autorun page that can't turn itself off → soft-lock |
| `unreachable-maps` | info | certain | map with no incoming (direct) transfer |
| `dead-common-event` | info | certain | common event never triggered or called — **opt-in** (`--dead-common-events`) |
| `orphan-assets` | info | certain | asset present but referenced nowhere — **opt-in** (`--orphans`), noisy on stock RTP |

Plugins (`js/plugins`):

| rule | severity | confidence | what it finds |
|---|---|---|---|
| `missing-base` | error | certain | a plugin's `@base` dependency is missing or disabled |
| `plugin-load-order` | error | certain | a plugin loads before a dependency it must load after |
| `unknown-plugin-command` | warning | certain/likely | a plugin command not registered by any enabled plugin (likely typo) |
| `plugin-conflict` | warning | likely | the same core method overwritten by several plugins (load-order dependent) |

The value is this **curated rule set**, which grows by real reports — every line is a bug or a real risk, not statistics.

## How it works

- **Local & deterministic.** The CLI does the full deterministic analysis on your machine; nothing is uploaded.
  Drop it on a project, run, read the console report.
- **Engine-agnostic core.** Analysis runs over an intermediate representation (IR) — a graph of entities, references,
  symbols and assets. A per-engine adapter translates RPG Maker JSON into the IR; the rules only ever see the IR. New
  engines plug in later via new adapters without rewriting the rules.
- **Doesn't run the game.** Event command lists are walked as an AST, never executed.

Workspace layout:

```
crates/core              dk-doctor-core      — IR + rules engine + findings (engine-agnostic)
crates/rpgmaker-adapter  dk-doctor-rpgmaker  — parses /data + interprets commands → IR
crates/cli               dk-doctor           — the binary: walk a project, run rules, render the report
```

## Desktop app (Tauri v2)

The desktop app (`apps/desktop`) wraps the Rust analyzer **as a library** (no subprocess) and renders the same
findings the CLI produces — its `analyze` command returns JSON byte-identical to `dk-doctor --format json`. It is a
standalone cargo project: its `src-tauri/Cargo.toml` declares its own empty `[workspace]`, so it is independent of the
root Cargo workspace and uses its own build dir (`apps/desktop/src-tauri/target`).

### Prerequisites
- Node 22+ and pnpm 10+ (`npm i -g pnpm`)
- Rust stable (MSVC on Windows). cargo must be on PATH:
  `$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"`
- Windows: WebView2 runtime (preinstalled on Win 11) + MSVC Build Tools.
- macOS: Xcode Command Line Tools (`xcode-select --install`). Mac is built only in CI.

### Run / build (Windows, local)
```powershell
cd apps/desktop
pnpm install                       # installs @tauri-apps/cli@^2 (Tauri CLI is a devDep)
pnpm tauri dev                     # dev with hot reload
pnpm tauri build                   # release -> src-tauri/target/release/bundle/nsis/*-setup.exe
```
The Tauri CLI is provided by the `@tauri-apps/cli` dev-dependency; `cargo-tauri` is not required.

### macOS build
Mac cannot be built on the Windows dev box. It is validated by CI on `macos-latest`
(`.github/workflows/desktop-build.yml`). To build locally on a Mac:
```bash
cd apps/desktop
pnpm install
pnpm tauri build -- --target universal-apple-darwin
```
**Signing/notarization is skipped for now** (ad-hoc). Unsigned macOS builds trigger Gatekeeper; right-click → Open, or
`xattr -dr com.apple.quarantine dk-doctor.app` to run. See [apps/desktop/README.md](apps/desktop/README.md) for the
full feature set, the health-score formula, and how to enable real signing later.

### CI
Pushes/PRs touching `apps/desktop/**` or `crates/**` run a `windows-latest` + `macos-latest` matrix that installs
node/pnpm/rust, builds the frontend, runs `tauri build`, and uploads the bundles as artifacts.

## Documentation

- [docs/rpgmaker-format-spec.md](docs/rpgmaker-format-spec.md) — RPG Maker MV/MZ data format reference (command
  parameter tables, reference/asset/symbol catalogues).
- [docs/architecture.md](docs/architecture.md) — Rust workspace, IR types, rules engine.
- [CONTRIBUTING.md](CONTRIBUTING.md) — how to report bugs and false positives, and build from source.

## License

**MIT** — see [LICENSE](LICENSE). Free and open source; you may use, modify, and redistribute it.

This is a **pre-release (beta)** build: results are advisory and may contain false positives or
miss real issues. The analyzer runs entirely locally and does not upload your project files or
telemetry. The desktop app can optionally check GitHub for a newer release on launch (toggle in
Settings); that check sends no project data.

Third-party open-source components (Tauri, Rust/JS libraries) remain under their own licenses.
