# dk-doctor rules reference

Every finding links here via its `remediation.docs_url` (`…/docs/rules.md#<rule-id>`).
Each section states **what** the rule detects, **why** it matters, and **how** to fix
it. Confidence is `certain` (static fact over the data) or `likely` (heuristic —
may be affected by plugins/scripts static analysis cannot see). Rules marked
*opt-in* are off by default and enabled with the noted flag.

The JSON artifact (`dk-doctor --format json`) attaches to each finding:

- `remediation.why` — one line on the impact, in the report language;
- `remediation.suggested_fix` — one concrete action, in the report language;
- `remediation.docs_url` — a deep link to this file;
- `fix` — *(only when safe)* a machine-applicable edit `{ kind, from, to }`. Today
  the sole kind is `asset_case_rename` (align an asset reference's letter case with
  the on-disk file — a meaning-preserving change).

---

## dead-variables

Detects a variable **written but never read** (dead state). Usually a forgotten
read or a typo in the variable id, so the write has no effect. Remove the write, or
add the intended read after checking the id. Confidence: `certain`.

## uninitialized-symbols

Detects a switch/variable **read but never written**. The value stays at its
default, so the guarded condition may never fire. Set the symbol before reading it,
or fix the id if the write targets a different one. When plugins are parsed, symbols
they declare (`@type switch/variable`) or set at runtime are excluded.
Confidence: `certain`.

## broken-transfer

Detects a **Transfer Player** whose destination map does not exist. A direct
transfer crashes the game when the event fires; a variable transfer (resolved by
constant propagation) crashes if the variable arrives with that value. Point the
transfer at an existing map, or create the missing one. Confidence: `certain`
(direct) / `likely` (variable).

## vehicle-start-map

Detects a boat/ship/airship whose **start map (System.json) is missing**. The
vehicle never spawns and cannot be boarded. Set an existing start map, or reposition
the vehicle in-game with a *Set Vehicle Location* (202) command. Confidence:
`likely`.

## unreachable-maps

Detects a map **not reachable by any direct transfer** from the start map —
possibly orphaned content. Add a transfer to it, or confirm it is opened by a
plugin / variable transfer (neither is tracked statically). Confidence: `likely`.

## referential-integrity

Detects a reference to a **database record that does not exist** (dangling id:
item/skill/enemy/troop/…). The reference fails at runtime. Create the record, or
repoint the reference to an existing id. Confidence: `certain`.

## broken-assets

Detects a reference to an **image/audio/video file missing from disk** — a black
image or silent audio at runtime. Add the file, or fix the referenced name.

A special case is a **case-only mismatch** (`asset_case_mismatch`): the file exists
but under different letter case. It loads on case-insensitive filesystems
(Windows/macOS) yet fails on case-sensitive ones (Linux servers, web builds) — so
this is reported as a *warning* (not an error) and carries a safe `fix` that aligns
the casing. Confidence: `certain`.

## orphan-assets

Detects an **on-disk file that nothing references** (possibly unused). Delete it if
truly unused, or confirm a plugin loads it (plugin references are not fully
tracked). *Opt-in* (`--orphans`): noisy on stock RTP. Confidence: `likely`.

## dead-code-after-exit

Detects commands **after an event exit** (Exit Event Processing) that can never run.
Labels reachable by a jump and the editor's trailing terminators are excluded.
Remove the unreachable commands, or move the exit. Confidence: `certain`.

## dead-self-switch

Detects a **self switch set but never checked** — the write has no effect. Add a
page condition on the self switch, or remove the write. Confidence: `certain`.

## unreachable-self-switch

Detects a page whose condition **requires a self switch that nothing ever sets** —
the page can never activate. Add a command that sets the self switch, or relax the
condition. Self switches set by plugins/scripts are not tracked. Confidence:
`certain`.

## dead-common-event

Detects a common event with **no trigger and no incoming caller** (command 117 /
effect 44) — it never runs. Give it a trigger, call it from an event, or delete it.
*Opt-in* (`--dead-common-events`): plugins often reserve common events dynamically.
Confidence: `likely`.

## cyclic-common-events

Detects common events that **call each other in a cycle** (command 117) — infinite
synchronous recursion that freezes/crashes the game. Break the cycle with a
switch/variable guard, or restructure the calls. Confidence: `certain`.

## shadowed-page

Detects an event page made unreachable by a **later page with looser conditions**:
RPG Maker picks the highest-index page whose conditions are met, so the later one
always wins. Tighten the later page's conditions, or reorder/remove the shadowed
page. Confidence: `certain`.

## stuck-autorun

Detects an **Autorun page that cannot turn itself off** (no self-switch/switch
write, transfer, common-event call, plugin command, or script). Autorun blocks input
while active, so the game soft-locks. Turn the gating switch/self-switch off inside
the page, or change the trigger. Confidence: `likely`.

## plugin-load-order

Detects a plugin loaded **before a dependency** it declares via `@base` /
`@orderAfter` / `@orderBefore`. It may initialize against missing code. Reorder the
plugins in Plugin Manager to satisfy the declared order. Confidence: `certain`.

## missing-base

Detects a plugin whose declared `@base` is **missing or disabled**. The plugin lacks
required code and most likely crashes on load. Add or enable the base plugin.
Confidence: `certain`.

## unknown-plugin-command

Detects a **plugin command that no enabled plugin registers** (via `@command` or
`registerCommand`). The engine silently skips it. Fix the command name, or
enable/add the plugin that provides it. MV (356) matching is best-effort; MZ (357) is
exact. Confidence: `certain` (MZ) / `likely` (MV).

## plugin-conflict

Detects a core method **overwritten by ≥2 plugins without an alias** — the later one
silently clobbers the earlier one's logic, so behaviour depends on load order. Check
compatibility and load order, or use a patch that keeps the original (alias).
Confidence: `likely` (AST heuristic).

## impossible-condition

Detects a *Conditional Branch* on a variable whose **value range makes the
comparison's result fixed** (symbolic propagation of Control Variables:
set/add/sub/random). One branch is dead code. Correct the value or the comparison,
or remove the dead branch. Confidence: `likely`.

## circular-gate

Detects a **progression deadlock**: switches that gate each other in a cycle, so
none can ever be turned on and the content behind them is unreachable. Break the
dependency so at least one switch can be set from outside the cluster. *Opt-in*
(`--circular-gates`, prototype): switches turned on by plugin commands are not
tracked. Confidence: `likely`.

## blocked-tile

Detects a **Transfer Player destination or the player start position on a tile
impassable from all four directions** — the player cannot move off it (soft-lock).
Move the destination to a passable tile, or adjust the tileset passability flags.
*Opt-in* (`--tiles`): passability plugins (region passage, pixel movement) are not
accounted for. Confidence: `likely`.

## picture-lifecycle

Detects a picture **operated on (Move/Rotate/Tint/Erase) before it is Shown** in the
same command list — the operation targets a picture that does not exist yet. Show
the picture first, or reorder the commands. *Opt-in* (`--pictures`): pictures persist
across events, so a show from another event/script is invisible. Confidence:
`likely`.

## empty-event-page

Detects an **empty Autorun page** (blocks input forever — a soft-lock) or an **empty
Parallel page** (runs every frame doing nothing — likely forgotten content). Add the
intended commands / a condition, change the trigger, or remove the page.
Confidence: `certain`.

## db-reachability

Detects a **database record referenced nowhere** in the data — enemy not in any
troop, skill not learnable, weapon/armor never sold/dropped/equipped/granted. Likely
unused content. Reference it where intended, or remove it. *Opt-in*
(`--db-reachability`): plugin/notetag references are not tracked. Confidence:
`likely`.
