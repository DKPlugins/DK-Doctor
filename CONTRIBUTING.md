# Contributing to dk-doctor

Thanks for your interest in **dk-doctor**, the static analyzer for RPG Maker MV/MZ projects.

The most valuable contribution right now is **feedback from real projects** — bug reports, false
positives, and "I wish it had caught this" stories. Those directly shape the rule set.

## Reporting issues

Please use the issue forms (they make triage much faster):

- **🐞 Bug report** — the analyzer crashed, failed to parse a project, or produced clearly wrong output.
- **🚩 False positive** — a finding that is actually fine in your project. dk-doctor's promise is that
  *every line of the report is a real bug or a real risk*, so false positives are treated as serious.
- **💡 Feature / new rule** — a class of bug you'd like dk-doctor to catch.

> **Never attach your game's assets or source.** dk-doctor runs entirely on your machine and never uploads
> anything. To reproduce an issue we only need a **minimal, redacted snippet** of the relevant
> `data/*.json`, event command, or `js/plugins.js` entry — not your project.

Before filing, please search existing issues to avoid duplicates.

## Building from source

Requires a [Rust toolchain](https://rustup.rs) (1.85+).

```sh
cargo build --release                     # build the CLI
cargo run -p dk-doctor -- "/path/to/project"
cargo test                                # run the test suite
cargo clippy --all-targets                # lint
cargo fmt                                 # format
```

Workspace layout and the design of the IR + rules engine are documented in
[docs/architecture.md](docs/architecture.md). The RPG Maker data-format reference the parser is built
against is in [docs/rpgmaker-format-spec.md](docs/rpgmaker-format-spec.md).

The desktop app lives in [apps/desktop](apps/desktop) (Tauri v2) — see its
[README](apps/desktop/README.md) for prerequisites and how to run it.

## Pull requests

Code PRs are welcome, but please **open an issue first** to discuss the approach — especially for new
rules, so we can agree on scope and the finding's message/confidence before you write code. A good new
rule is: cheap, deterministic where possible, and honest about its `confidence` level.

If you submit a PR, keep it focused, run `cargo test` / `cargo clippy` / `cargo fmt`, and match the
surrounding code style. New finding messages go through the structured `Msg` catalogue (RU/EN), not
inline strings — see existing rules for the pattern.

## License note

dk-doctor is currently a proprietary pre-release (beta) build — see [EULA.md](EULA.md). By submitting a
contribution you agree that it may be included in the project under that license.
