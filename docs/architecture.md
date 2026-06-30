# dk-doctor — Rust Workspace + IR + Rules Engine Architecture

> Engine-agnostic deterministic core + RPG Maker adapter + CLI report, shipped as one fast `.exe`
> (and embedded as a library by the desktop app). The IR is intentionally shaped so new engine
> adapters and new rules plug in without refactoring the core. Findings are `certain` or `likely`.
> Dev + CI platform is Windows (MSVC); call-outs noted.

---

## 1. Cargo workspace layout

Virtual workspace (root has `[workspace]`, no `[package]`). **One-way dependency rule, structurally enforced:**
core depends on nothing engine-specific; adapter depends on core; cli depends on both. This is the multi-engine
guardrail — a future `godot-adapter` produces the same `Ir` and every rule runs unchanged. Command codes
`201/117/121…` exist **only** inside the adapter.

```
dk-doctor/
  Cargo.toml                # [workspace] + [workspace.dependencies] (single version source)
  rust-toolchain.toml       # pin toolchain (MSVC) for reproducible Windows/CI builds
  crates/
    core/                   # dk-doctor-core (lib) — engine-AGNOSTIC, zero RPG Maker vocabulary
      src/
        lib.rs
        ir/{mod,entity,edge,graph,symbols,location,asset}.rs
        finding.rs
        rules/{mod, dead_variables, uninit_symbols, broken_transfer, unreachable_maps,
               referential_integrity, broken_assets, orphan_assets, dead_code_after_exit}.rs
        report.rs
    rpgmaker-adapter/       # dk-doctor-rpgmaker (lib) — depends on core
      src/
        lib.rs              # pub fn load_project(root) -> Result<Ir, AdapterError>
        raw/{mod, map, database, common_event, system, plugins}.rs   # serde for /data/*.json
        command.rs          # EventCommand { code:u16, indent:i32, parameters: Vec<Value> }
        codes.rs            # command code consts + param-index helpers
        interpreter.rs      # command-list walker -> edges + symbol sites (the codes->IR map)
        assets.rs           # scan img/ audio/ effects/ movies/ -> manifest + collect refs
        build.rs (module)   # raw -> Ir builder, engine detection, www/ fallback
    cli/                    # dk-doctor (bin) — the shipped binary
      src/{main, args}.rs
      src/render/{mod, console, json}.rs
  testdata/                 # sample MV + MZ fixtures for integration + insta snapshots
  xtask/ (optional)         # cargo xtask dist -> single stripped .exe
```

Crate `name`s: `dk-doctor-core`, `dk-doctor-rpgmaker`, `dk-doctor` (bin `name = "dk-doctor"`).

### Root `Cargo.toml`

```toml
[workspace]
resolver = "3"
members  = ["crates/core", "crates/rpgmaker-adapter", "crates/cli"]

[workspace.package]
edition = "2024"          # stabilized 1.85; installed toolchain 1.96 supports it
license = "MIT OR Apache-2.0"

[workspace.dependencies]
serde       = { version = "1", features = ["derive"] }
serde_json  = "1"
thiserror   = "2"
rustc-hash  = "2"                                   # FxHashMap/FxHashSet for hot indexes
camino      = { version = "1", features = ["serde1"] }
petgraph    = "0.8"                                 # OPTIONAL; current rules don't require it
ignore      = "0.4"                                 # parallel walker (preferred over walkdir)
walkdir     = "2"                                   # simpler fallback
clap        = { version = "4", features = ["derive"] }
miette      = { version = "7", features = ["fancy"] }
owo-colors  = "4"
anstream    = "1"

[workspace.dev-dependencies]
insta       = "1"                                   # snapshot tests against testdata/
```

> Pin exact versions at scaffold by reading crates.io; the major-version floors above are safe. Core's manifest
> lists **only** `serde, serde_json, thiserror, rustc-hash, camino` (+ optional `petgraph`) — no `clap`, no
> `ignore`/`walkdir`, no adapter. That omission is the structural guarantee core stays agnostic.

---

## 2. IR design (`dk-doctor-core`)

Build-once / query-many. `EntityId(u32)` indexes a `Vec<EntityNode>` arena. Edges are a flat `Vec<EdgeRecord>`
plus precomputed `FxHashMap` indexes, because rules need **typed** queries ("all Transfer edges", "all writes of
switch 14") that a generic graph doesn't index for free. Hand-rolled BFS covers unreachable-maps, so `petgraph`
stays optional.

### Location (engine-agnostic breadcrumb)

```rust
// ir/location.rs
use camino::Utf8PathBuf;

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct Location { pub file: Utf8PathBuf, pub path: LocationPath }   // file = "data/Map003.json"

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct LocationPath(pub Vec<PathSeg>);                              // renders "Map003/EV005/page2/cmd14"

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathSeg {
    Map(u32), Event(u32), Page(u32), Command(u32),
    CommonEvent(u32), Troop(u32),
    DbRecord { file: &'static str, id: u32 },
    Plugin(String), Line(u32),          // reserved for future AST layer
}
```

### Entities

```rust
// ir/entity.rs
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub struct EntityId(pub u32);

pub struct EntityNode { pub id: EntityId, pub kind: Entity, pub location: Location }

pub enum Entity {
    Map(Map), Event(Event), Page(Page), CommonEvent(CommonEvent), Troop(Troop),
    DatabaseRecord(DatabaseRecord), Asset(AssetRef), Script(ScriptBlackbox),
}

pub struct Map         { pub map_id: u32, pub name: String, pub event_ids: Vec<EntityId> }
pub struct Event       { pub map_id: u32, pub event_id: u32, pub page_ids: Vec<EntityId> }
pub struct Page        { pub conditions: PageConditions, pub command_count: u32 }
pub struct CommonEvent { pub id: u32, pub name: String, pub trigger: CeTrigger }
pub struct Troop       { pub id: u32 }
pub struct DatabaseRecord { pub kind: DbKind, pub record_id: u32, pub name: String }
pub struct ScriptBlackbox { pub source: String }        // 355/655 / plugin body, not parsed iter1

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DbKind { Actor, Class, Skill, Item, Weapon, Armor,
                  Enemy, Troop, State, Animation, Tileset, CommonEvent }

/// Page conditions = READ sites; Option = None when the matching *Valid flag is false.
pub struct PageConditions {
    pub switch1: Option<u32>, pub switch2: Option<u32>,
    pub variable: Option<u32>, pub variable_value: Option<i64>,
    pub self_switch: Option<char>, pub item: Option<u32>, pub actor: Option<u32>,
}
```

### Edges

```rust
// ir/edge.rs
pub struct EdgeRecord { pub from: EntityId, pub edge: Edge, pub location: Location }

pub enum Edge {
    Transfer { to_map: Option<u32>, designation: TransferDesignation },  // 201; None = by-variable
    CallsCommonEvent { common_event_id: u32 },                           // 117
    ReadsSwitch  { switch_id: u32 },  WritesSwitch { switch_id: u32 },
    ReadsVariable { variable_id: u32 }, WritesVariable { variable_id: u32 },
    ReferencesAsset { asset: AssetKey },                                 // 101/231/241.../322...
    ReferencesDbId { kind: DbKind, id: u32 },                            // 126/127/128/301/311-325...
}

#[derive(Copy, Clone, Debug)]
pub enum TransferDesignation { Direct, ByVariable }                      // 201 [0]==0 vs ==1
```

### Symbols

```rust
// ir/symbols.rs
use rustc_hash::FxHashMap;

pub struct Site { pub location: Location, pub entity: EntityId }

#[derive(Default)]
pub struct SymbolInfo {
    pub id: u32,
    pub name: Option<String>,            // from System.switches/variables ("" name => Some(""))
    pub reads: Vec<Site>,
    pub writes: Vec<Site>,
    pub declared_by_plugin: bool,        // ALWAYS false in iter1; field exists so uninit rule is final
}

#[derive(Default)]
pub struct SymbolTable {
    pub switches:  FxHashMap<u32, SymbolInfo>,
    pub variables: FxHashMap<u32, SymbolInfo>,
    pub max_switch_id: u32,               // from System.switches.len()-1; defines valid range
    pub max_variable_id: u32,
}
```

### Graph + indexes + assets

```rust
// ir/graph.rs
use rustc_hash::{FxHashMap, FxHashSet};

pub struct Ir {
    pub engine: Engine,                                  // Mv | Mz (detected by adapter)
    pub entities: Vec<EntityNode>,
    pub edges: Vec<EdgeRecord>,
    pub symbols: SymbolTable,
    pub start_map_id: Option<u32>,                       // System.startMapId (root for reachability)
    pub maps_by_id: FxHashMap<u32, EntityId>,
    pub common_events_by_id: FxHashMap<u32, EntityId>,
    pub db: FxHashMap<DbKind, FxHashMap<u32, EntityId>>, // existence checks
    pub assets_present: FxHashSet<AssetKey>,             // files actually on disk (post encryption-aware norm)
    pub asset_refs: Vec<(AssetKey, Location)>,           // every reference site
}
impl Ir {
    pub fn edges_from(&self, e: EntityId) -> impl Iterator<Item = &EdgeRecord>;
    pub fn db_exists(&self, kind: DbKind, id: u32) -> bool;
}

// ir/asset.rs
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
pub struct AssetKey { pub kind: AssetKind, pub name: String }   // bare name, no ext; kind disambiguates folder
pub type AssetRef = AssetKey;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    Face, Character, Picture, Parallax, Tileset, Battleback1, Battleback2,
    Title1, Title2, Enemy, SvEnemy, SvActor, Animation, Effect, Movie,
    Bgm, Bgs, Me, Se,
}
```

### Finding (stable `rule` id + structured message)

```rust
// finding.rs
#[derive(Clone, Debug, serde::Serialize)]
pub struct Finding {
    pub severity: Severity, pub category: Category, pub confidence: Confidence,
    pub location: Location, pub message: String,
    pub references: Vec<Location>,         // related sites (other read pages, write sites, etc.)
    pub rule: &'static str,                // stable id, e.g. "broken-transfer"
}
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity { Info, Warning, Error }   // Ord => sort error-first (reverse)
#[derive(Copy, Clone, Debug, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category { Data, Reference, Asset, DeadCode, PluginOrder, PluginConflict } // last two reserved
#[derive(Copy, Clone, Debug, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence { Certain, Likely }
```

---

## 3. Adapter: serde strategy + interpreter

### Hybrid parsing (the central messiness: third-party project drift)

- **Typed structs** for the stable analysis spine only: `MapInfo`; the event/page/command skeleton;
  `System.switches`/`variables` + asset fields; each DB record's `id`+`name`+the specific FK fields. Use
  `#[serde(default)]` on optionals; **never** `deny_unknown_fields` (plugin-injected fields must be tolerated).
- **`serde_json::Value`** for opaque payloads: full command `parameters` (`Vec<Value>`, read positionally with
  helpers like `p.get(1).and_then(Value::as_u64)`), plugin parameter blobs, notetag strings. Typing parameters
  per-code is over-engineering for iteration 1.
- **1-based-array-with-null-at-0**: deserialize tables as `Vec<Option<T>>` (preserves `index == record_id`,
  tolerates interior null holes from deleted records). The leading null and holes become `None` — skip them.
- **Resilience:** parse each file independently; a parse error on one `MapXXX.json` becomes a `Finding`/diagnostic,
  not a whole-run crash. Canonical envelope: `EventCommand { code: u16, indent: i32, parameters: Vec<Value> }`
  (typed envelope, untyped payload).
- **plugins.js**: strip `var $plugins =` / trailing `;`, then `JSON.parse`. Iteration 1 captures names/status/order
  only; struct-param decode and `@param` interpretation are Layer A (deferred).

### Engine detection + layout (`build.rs` module)

Try project root, fall back to `www/`. Detect MZ vs MV by: presence of `effects/` folder / `command357` usage in
event lists / MZ-only System fields. Set `Ir.engine`. Branch encryption-suffix resolution and Animations format
per engine/per-entry. `interpreter.rs` is the **only** place command codes map to edges/sites — exactly per the
format-spec parameter table; it must validate each code's indices against real MV and MZ sample data in `testdata/`.

---

## 4. Rules engine + iteration-1 rules

```rust
// rules/mod.rs
pub struct RuleCtx<'a> { pub ir: &'a Ir, pub plugin_decls_available: bool /* false in iter1 */ }

pub trait Rule: Send + Sync {
    fn id(&self) -> &'static str;
    fn category(&self) -> Category;
    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding>;
}

pub struct Registry { rules: Vec<Box<dyn Rule>> }
impl Registry {
    pub fn with_builtin() -> Self { /* Box::new each rule below */ }
    pub fn run_all(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        self.rules.iter().flat_map(|r| r.run(ctx)).collect()   // .par_iter() later; deterministic sort after
    }
}
```

Rules take `&Ir`, never mutate, emit `Vec<Finding>`. Adding a rule = one file + one `Box::new(...)` line (the
incremental-by-real-reports guardrail). Runner sorts error-first then by location for stable output / `insta` snapshots.

### Iteration-1 rule list (ordered: ship `dead-variables` first)

| # | rule id | category | severity | confidence | logic |
|---|---|---|---|---|---|
| 1 | `dead-variables` | data | warning | certain | var has ≥1 `WritesVariable` site, **0** `ReadsVariable` → written never read. Write sites in `references`. Pure SymbolTable query — cheapest, first to ship/validate. |
| 2 | `uninitialized-symbols` | data | warning | **likely** | switch/var with ≥1 read, **0** writes → condition can never become true / value never set. Confidence softened to `likely` with disclaimer ("not cross-checked against plugin @param") until Layer-A sets `declared_by_plugin`; suppress when `declared_by_plugin`, then promote to `certain`. Reads in `references`. |
| 3 | `broken-transfer` | reference | error | certain | `Edge::Transfer { to_map: Some(id), Direct }` where `id ∉ maps_by_id` → 201 to missing map = crash on trigger. `ByVariable` → skip (runtime-computed). |
| 4 | `unreachable-maps` | reference | **info** | certain | BFS from `start_map_id` over `Transfer` (direct) edges only; map with 0 incoming direct transfers and not start → unreachable. **Iteration-1 decision: severity `info`, message notes it may be reached via variable-transfer / plugin / common event** (open Q resolved leniently to cut false positives). |
| 5 | `referential-integrity` | reference | error | certain | each `Edge::ReferencesDbId{kind,id}` (126/127/128 items, 301 troop, 311–325 actor/skill/state/class ops, 336 enemy) → `ir.db_exists(kind,id)`; missing → dangling reference. `O(edges)`. |
| 6 | `broken-assets` | asset | error | certain | every `asset_refs` `AssetKey ∉ assets_present` → reference to a file not on disk (encryption-aware presence normalized in adapter). |
| 7 | `orphan-assets` | asset | info | certain | reverse set-difference: file under known asset folder with 0 refs → unused. Apply §4.5 guards (system set, ogg/m4a pair, `$`/`!` names, effects/ transitive). **NOTE: plugins not parsed in iter1 → plugin-referenced assets are NOT yet known; word findings as "possibly unused" and log that plugin refs are not yet accounted.** |
| 8 | `dead-code-after-exit` | dead-code | warning | certain | within a page command list, commands at same indent after a `115` Exit with no intervening label/branch re-entry → unreachable. Needs per-page command order+indent. |

Implementability notes: rules 1–2 are pure `SymbolTable` lookups; 3,5,6,7 are `edges`/set lookups; 4 is one BFS;
8 is the only rule needing the per-page command sequence (retain it on `Page`/in the interpreter pass).

---

## 5. CLI flow + Windows call-outs

`main.rs`: `clap` parse → `rpgmaker::load_project(root)` → `Ir` → `Registry::with_builtin().run_all(&ctx)` → sort →
`render::console` (miette) or `render::json` (serde_json `Report`) → exit code (`0` clean / `1` warnings / `2` errors,
CI-usable). Subcommands `check` / `explain`; flags `--format json|console`, `--min-severity`, `--root`,
`--enable`/`--disable` (filter by `rule` id).

**Console renderer = miette (`fancy`), not ariadne:** ariadne renders **source-code spans** (byte offsets/carets);
dk-doctor locations are logical JSON breadcrumbs (`Map003/EV005/page2/cmd14`) the user never hand-edits — there's no
span to underline. miette's `Diagnostic` cleanly carries `severity` + `code` (= `rule` id) + message + optional labels
(we omit labels), with built-in color/width/`NO_COLOR`/pipe handling. `owo-colors`+`anstream` for the summary banner.

Windows: `rust-toolchain.toml` pins MSVC+version (reproducible across dev box and CI); **camino `Utf8PathBuf`**
everywhere user-facing (clean JSON serialization of backslash/non-ASCII project paths, common under Windows user
dirs); **miette `fancy` + anstream** give correct VT/ANSI on Windows Terminal/conhost and auto-strip when piped
(clean `--format json`) — the concrete reason to avoid hand-rolled `\x1b[` codes that misrender in old `cmd.exe`;
`ignore`/`walkdir` need only filenames (assets never read) so the walk is metadata-only and cheap; optional
`xtask dist` produces the single stripped `.exe`.
