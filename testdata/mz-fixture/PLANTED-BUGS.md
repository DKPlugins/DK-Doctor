# Planted bugs — mz-fixture

Synthetic, minimal but valid RPG Maker **MZ** project (engine detected as `mz` via
`advanced` block + a `357` plugin command in `CommonEvents.json`). Each rule from
iteration 1 has exactly one clearly planted bug plus a valid control case that must
**not** be flagged.

Symbol table (`System.json`):
- switches: `#1 DoorOpened`, `#2 BossDefeated`, `#3 PluginGate`
- variables: `#1 Gold`, `#2 DeadCounter`

Plugin layer (`js/plugins.js` + `js/plugins/*.js`) — Tier A. Load order (enabled):
`GateCore` → `DependentPlugin` → `LatePlugin`; `DisabledPlugin` is `status:false`.
- `GateCore`: `@param GateSwitch @type switch` = **3** (owns switch `#3 PluginGate`);
  registers `@command openGate`.
- `DependentPlugin`: `@base GhostBase` (absent) + `@orderAfter LatePlugin`
  (LatePlugin loads later → wrong order).
- `LatePlugin`: plain (only the *target* of the order violation).
- `DisabledPlugin`: disabled → not in load order, `@command unused` not registered,
  file not even read.

Maps: `#1 Town` (start), `#2 Cave` (has `encounterList` → `can_battle`), `#3 SecretRoom`
(no encounters / no `301` → `!can_battle`). Map `#99` does not exist.

Common events (`CommonEvents.json`): `#1 Init` (Autorun), `#2 Orphan` (trigger None),
`#3 LoopA` (trigger None, `117`→#4), `#4 LoopB` (trigger None, `117`→#3). CE `#99` absent.

DB records present: Class #1, Skill #1, Item #1, Weapon #1, Armor #1, State #1,
Enemy #1, Animation #1, Tileset #1, Actor #1. Ids `#99` absent everywhere.

| Rule | Planted bug — exact location | Why it fires |
|---|---|---|
| `dead-variables` | `Map001` / EV002 "Logic" / page1 / cmd3 — `122` writes variable **#2 DeadCounter** | written once, never read anywhere |
| `uninitialized-symbols` | `Map001` / EV001 "Greeter" / page1 condition + EV002 / page1 / cmd5 (`111`) read switch **#2 BossDefeated** | read 2x, never written by any `121`. plugins.js разобран → сверено с `@param` → confidence **Certain**, `plugin_checked:true` |
| `missing-base` | `DependentPlugin` `@base GhostBase` | GhostBase отсутствует в `plugins.js` (`disabled:false`) |
| `plugin-load-order` | `DependentPlugin` `@orderAfter LatePlugin` | LatePlugin грузится ПОСЛЕ DependentPlugin → требование «раньше» нарушено |
| `broken-transfer` | `Map001` / EV001 "Greeter" / page2 / cmd1 — `201` direct transfer to **map #99** | map #99 has no `Map099.json` / no `MapInfos` entry |
| `unreachable-maps` | `Map003` "SecretRoom" | no incoming direct `201` transfer; not the start map |
| `referential-integrity` (×5) | see breakdown below | five `Edge::ReferencesDbId` whose target is absent |
| `broken-assets` (×3) | `Map002` / EV001 "Chest" / page1 / cmd1 — `231` Show Picture **"GhostPic"**; `Map002` `battleback1Name` **"MissingArena"**; `Tilesets.json` #1 "Outside" image slot **"Outside_B"** | `img/pictures/GhostPic.png` absent; `img/battlebacks1/MissingArena.png` absent AND Map002 has `encounterList` → `can_battle` → battleback usage-gating does NOT suppress it; `img/tilesets/Outside_B.png` absent AND tileset #1 is USED by Map001/Map002 → tileset usage-gating does NOT suppress it |
| `stuck-autorun` | `Map002` / EV002 "Loop" / page1 — Autorun (`trigger:3`) gated on switch **#1**, body only `101`/`401` (text) | gated Autorun with no self-switch/switch write and no transfer → infinite autorun (soft-lock) |
| `shadowed-page` | `Map001` / EV001 "Greeter" / page1 (needs switch **#1**+switch **#2**) shadowed by page2 (no conditions) | RM picks highest-index page whose conditions hold; page2 always active → page1 unreachable |
| `orphan-assets` | `img/pictures/Unused.png` | present on disk, referenced nowhere (CLI: opt-in via `--orphans`) |
| `dead-code-after-exit` | `Map001` / EV002 "Logic" / page1 — cmds after `115` Exit at indent 0 (cmd9 `101`, cmd10 `108`, cmd11 blank) | unreachable: the `115` at cmd8 ends event processing |
| `dead-self-switch` | `Map001` / EV003 "SwitchLogic" / page1 / cmd1 — `123` sets self-switch **B** | written once, never read by any page condition / `111` type 2 |
| `unreachable-self-switch` | `Map001` / EV003 "SwitchLogic" / page2 condition — requires self-switch **D** | no `123` on EV003 ever sets D → page unreachable (plugin caveat) |
| `dead-common-event` | `CommonEvents.json` / CE **#2 "Orphan"** | trigger None, no incoming `117` and no effect-44 reference (CLI: opt-in via `--dead-common-events`) |
| `cyclic-common-events` | `CommonEvents.json` / CE **#3 ↔ #4** | mutual `117` calls form a cycle (canonical `[3,4]`) |
| `unknown-plugin-command` | `CommonEvents.json` / CE **#1 "Init"** — `357` call `("DummyPlugin","doNothing")` | DummyPlugin is not in `plugins.js`, so the pair isn't in any `@command` registry (structured 357) |

### `referential-integrity` breakdown (5 findings)

| Source | Target (missing) | DbKind |
|---|---|---|
| `Map002` EV001 page1 cmd0 — `126` Change Items | item **#99** | Item |
| `Map003` `tilesetId` | tileset **#99** | Tileset |
| `Classes.json` Class #1 `learnings[1].skillId` | skill **#99** | Skill |
| `Enemies.json` Enemy #1 `dropItems[1]` (kind 1) | item **#99** | Item |
| `Items.json` Item #1 `effects[1]` (effect 44) | commonEvent **#99** | CommonEvent |

## Control cases (must NOT be flagged)

- switch **#1 DoorOpened**: written (`121`, EV002 cmd0) and read (page condition) → not uninitialized.
- variable **#1 Gold**: written (`122`, EV002 cmd1) and read (`111`, EV002 cmd2) → not dead.
- `201` direct transfer to **map #2** (EV001 page2 cmd0): target exists → not broken.
- `126` Change Items **item #1 Potion** (Map002 cmd2): exists → no dangling ref.
- DB FK controls that resolve: Class #1 `learnings[0]` → skill **#1**; Enemy #1 `dropItems[0]`
  → item **#1**; Item #1 `effects[0]` (effect 21) → state **#1**; Weapon #1 `animationId` → animation **#1**;
  Actor #1 `classId` → class **#1**, `equips[0]` → weapon **#1**. None flagged.
- self-switch **C** on EV003: written (`123`, page1 cmd0) **and** read (page1 condition) → neither
  dead nor unreachable.
- CE **#1 "Init"** (Autorun): runs by trigger even though uncalled → not a dead common event.
- CE **#3/#4**: each is `117`-called by the other → not dead common events (the cycle is the bug).
- Asset `Actor1` face (`101`), enemy `Slime` (`img/enemies` + `img/sv_enemies`),
  effect `Explosion` (`effects/Explosion.efkefc`): present + referenced → not broken / not orphan.
- The inner `115` Exit at indent 1 (EV002 cmd6): sits inside a conditional branch; the
  code after the branch closes (indent drops) stays alive → no dead-code finding there.
- Effect assets are exempt from `orphan-assets` (transitive `.efkefc` dependencies).
- **battleback usage-gating control:** `Map003` `battleback1Name` **"NoBattleHere"** is absent
  from disk, but Map003 has no `encounterList` and no `301` (`!can_battle`) → battleback never
  loads → NOT flagged by `broken-assets`.
- **stuck-autorun control:** `Map002` EV003 "GoodLoop" page1 is an Autorun gated on switch #1, but
  page1 writes self-switch **'A'** (`123`) → legitimate "autorun → set self-switch → next page"
  pattern → NOT flagged. (Its self-switch 'A' is read by page2 condition → not dead/unreachable.)
- **stuck-autorun Tier-A control:** `Map002` EV004 "PluginLoop" page1 is an Autorun gated on
  switch **#3 PluginGate**, body only text (no exit) — would normally be a soft-lock, BUT switch #3
  is declared by plugin `GateCore` (`@param ... @type switch` = 3) → the plugin drives it at
  runtime → **SUPPRESSED** (the whole point of Tier A). So `stuck-autorun` stays at **1**, not 2.
- **uninitialized-symbols promotion:** because `plugins.js` parses, the lone uninit (switch #2)
  is cross-checked against every enabled plugin's `@param` and confirmed not plugin-owned →
  promoted to `Certain` with `plugin_checked:true`.
- **plugin controls:** `LatePlugin` declares no constraints → not flagged (only the order
  *target*). `DisabledPlugin` (`status:false`) is absent from the load order → its `@command unused`
  is not registered and `missing-base`/`plugin-load-order` ignore it.

## Expected verdict

Running all built-in rules (`Registry::with_builtin().run_all`, incl. `orphan-assets`):
**11 errors, 12 warnings, 3 infos**. Process exit code `2` (errors present).

- errors (11): `referential-integrity` ×5 + `broken-transfer` + `broken-assets` ×3
  (GhostPic + MissingArena + Outside_B) + `missing-base` + `plugin-load-order`.
- warnings (12): `uninitialized-symbols` + `dead-variables` + `dead-code-after-exit` ×3 +
  `dead-self-switch` + `unreachable-self-switch` + `cyclic-common-events` + `shadowed-page` +
  `stuck-autorun` + `unknown-plugin-command` + `impossible-condition`.
- infos (3): `unreachable-maps` + `orphan-assets` + `dead-common-event`.

CLI default (`--orphans` off) hides the single `orphan-assets` info → 11 errors / 12 warnings / 2 infos.
