# dk-doctor — RPG Maker MV/MZ DATA Format Reference (authoritative, build-ready)

> Single source for the parser + IR + adapter. Verified against MV `rpg_objects/Game_Interpreter.js`
> (rpgtkoolmv/corescript, per-class split — no monolithic `js/rpg_objects.js` on master), MZ
> `rmmz_objects.js` / `rmmz_managers.js` / `rmmz_core.js` / `rmmz_sprites.js` (stak/rmmz-corescript),
> plus real serialized `/data/*.json` from MV (Apress/beg-rpg-maker-mv) and MZ (nz-prism/RPG-Maker-MZ).
> **Index meanings are identical MV↔MZ for every command below except 101 (MZ adds `[4]` speaker) and the
> plugin-command split (356 MV string vs 357 MZ struct).**

---

## 0. Universal conventions

- Every DB file **except `System.json` and `MapInfos.json`** is a flat JSON array: `[null, {id:1}, {id:2}, …]`.
  **Index 0 is always `null`. IDs are 1-based and equal the array index.** Interior `null` holes occur
  (deleted records) — always filter nulls and trust the object's `id`.
- A reference value of `0` means **"none/unset"** and is NOT a dangling reference. Skip it.
- `System.switches` / `System.variables` are arrays of **strings (names)**: index 0 is the empty string `""`
  (not `null`); `arr[id]` is the display name; **array length defines the valid 1-based ID range.** An empty
  string name = declared-but-unnamed slot (still valid to reference).
- `MapInfos.json` is a **sparse array indexed by map id** (index 0 = null); entries `{id,name,parentId,order,…}`.
- `MapXXX.json` is a **single object**; its `events[]` is a sparse 1-indexed array (null at 0; `event.id` == index).
- All JSON is UTF-8, no BOM, minified. Almost every object carries a free-text `note` (notetags — opaque to
  iteration-1 static rules; route to plugin/AST layer later).
- `traits[]` = `{code,dataId,value}`; `effects[]` = `{code,dataId,value1,value2}`. **`dataId` is a typed
  foreign key whose target depends on `code`** (tables in §3.2/§3.3).
- Event command on disk: `{ code:int, indent:int, parameters:Array }`. Move-route commands (codes 1–47) live
  inside `page.moveRoute.list` and are a **separate** instruction set, not event commands.

---

## 1. THE EVENT-COMMAND PARAMETER-INDEX TABLE (most important artifact)

`p[i]` == on-disk `command.parameters[i]`. MV reads `this._params[i]`; MZ receives `params` arg — same array.
"R/W" columns describe what the analyzer must track.

### 1.1 Message / choices / flow control

| code | name | parameters[] | notes |
|---|---|---|---|
| 101 | Show Text (header) | `[0]`=faceName (img/faces, ""=none), `[1]`=faceIndex, `[2]`=background(0 win/1 dim/2 transparent), `[3]`=positionType(0 top/1 mid/2 bottom). **MZ ONLY** `[4]`=speakerName (string). | MV arrays len 4, MZ len 5. `[0]` is a **face asset ref**. Body lines follow as 401. |
| 401 | Text line (cont.) | `[0]`=text string | loops while next code===401. May embed `\V[n]`,`\N[n]`,`\C[n]`,`\I[n]` escapes (route to AST later). |
| 102 | Show Choices (header) | `[0]`=choices `Array<string>`, `[1]`=cancelType(-2 disallow/-1 branch/0..n), `[2]`=defaultType, `[3]`=positionType(0 L/1 M/2 R), `[4]`=background. `[2..4]` length-guarded in MV. | Arms opened by 402; cancel by 403; closed by 404 (no method). |
| 402 | When [choice n] | `[0]`=choice index (load-bearing), `[1]`=choice text | branch arm; `if _branch[indent]!==[0] skipBranch()`. |
| 403 | When Cancel | (none) | branch arm for index<0. |
| 404 | End of Choices | (none) | **no `command404` method** — structural terminator. |
| 103 | Input Number | `[0]`=variableId (**WRITE**), `[1]`=maxDigits(1..8) | |
| 104 | Select Item | `[0]`=variableId (**WRITE**, receives item id), `[1]`=itemType/category | MZ category enum not fully pinned (open Q). |
| 105 | Show Scrolling Text | `[0]`=speed, `[1]`=noFast | body lines as 405. |
| 108 / 408 | Comment / Comment line | `[0]`=comment text | notetag-style plugin calls often live here. |
| 111 | Conditional Branch | `[0]`=condition TYPE (0..13) — sub-layout §1.2 | sets `_branch[indent]`; skipBranch if false. |
| 411 | Else | (none) | `if _branch[indent]!==false skipBranch()`; same indent as its 111. |
| 412 | End Branch | (none) | **no method** — structural. |
| 112 / 413 / 113 | Loop / Repeat Above / Break Loop | (none) | 413 walks index backward to matching 112; 113 scans forward past matching 413. |
| 115 | Exit Event Processing | (none) | `_index = _list.length` → anything after at reachable indent is **dead code**. |
| 117 | Common Event | `[0]`=**commonEventId → CommonEvents** (graph edge) | broken if id missing/out of range. |
| 118 / 119 | Label / Jump to Label | `[0]`=label name (string) | |

### 1.2 Conditional Branch (111) condition-type sub-layout — `[0]` selects

| `[0]` type | meaning | sub-params |
|---|---|---|
| 0 | Switch | `[1]`=**switchId (READ)**, `[2]`=expected(0 ON/1 OFF) |
| 1 | Variable | `[1]`=**variableId (READ)**, `[2]`=src(0 const/1 variable), `[3]`=value OR **variableId (READ if [2]==1)**, `[4]`=op(0 ==,1 ≥,2 ≤,3 >,4 <,5 ≠) |
| 2 | Self Switch | `[1]`=ch ("A".."D") (READ), `[2]`=expected |
| 3 | Timer | `[1]`=seconds, `[2]`=cmp(0 ≥/1 ≤) |
| 4 | Actor | `[1]`=**actorId**, `[2]`=check(0 in party,1 name,2 class,3 skill,4 weapon,5 armor,6 state), `[3]`=operand: name string / **classId→Classes** / **skillId→Skills** / **weaponId→Weapons** / **armorId→Armors** / **stateId→States** |
| 5 | Enemy | `[1]`=troop member index, `[2]`=check(0 appeared/1 state), `[3]`=**stateId→States** |
| 6 | Character | `[1]`=characterId(-1 player/0 this event/>0 eventId), `[2]`=direction |
| 7 | Gold | `[1]`=amount, `[2]`=cmp(0 ≥/1 ≤/2 <) |
| 8 | Item | `[1]`=**itemId→Items** |
| 9 | Weapon | `[1]`=**weaponId→Weapons**, `[2]`=includeEquip |
| 10 | Armor | `[1]`=**armorId→Armors**, `[2]`=includeEquip |
| 11 | Button | `[1]`=key name; **MZ** adds `[2]`=press mode(0 pressed/1 triggered/2 repeated) |
| 12 | **Script** | `[1]`=raw JS string (`!!eval`) → **opaque, route to AST** |
| 13 | Vehicle | `[1]`=vehicleType(0 boat/1 ship/2 airship) |

### 1.3 Switch / variable / self-switch (symbol-table write/read sites)

| code | name | parameters[] |
|---|---|---|
| 121 | Control Switches | `[0]`=startId, `[1]`=endId, `[2]`=value(0 ON/1 OFF) → **WRITES switch range [start..end] inclusive** |
| 122 | Control Variables | `[0]`=startId, `[1]`=endId, `[2]`=operation(0 set,1 add,2 sub,3 mul,4 div,5 mod), `[3]`=operand TYPE → **WRITES var range**. Operand: 0 const→`[4]`=value; 1 variable→`[4]`=**srcVarId (READ)**; 2 random→`[4]`=min,`[5]`=max; 3 gameData→`[4]`,`[5]`,`[6]`=(type,p1,p2) per `gameDataOperand`; 4 script→`[4]`=raw JS string (`eval`, **opaque**) |
| 123 | Control Self Switch | `[0]`=ch("A".."D"), `[1]`=value(0 ON/1 OFF) → **WRITES self-switch** keyed `[mapId,eventId,ch]`; only if eventId>0 |
| 124 | Control Timer | `[0]`=op, `[1]`=seconds |

`gameDataOperand(type=[4], p1=[5], p2=[6])` (122 operand type 3): 0 Item count (p1=**itemId→Items**); 1 Weapon
count (p1=**weaponId→Weapons**); 2 Armor count (p1=**armorId→Armors**); 3 Actor (p1=**actorId→Actors**, p2 selects
level/exp/hp/mp/tp/param); 4 Enemy (p1=troop member index, p2 stat); 5 Character (p1=charId, p2 x/y/dir/screenXY);
6 Party (p1=member index → actorId); 7 Other (0 mapId,1 partySize,2 gold,3 steps,4 playtime,5 timer,6 saveCount,
7 battleCount,8 winCount,9 escapeCount); 8 Last-action data. Only Item/Weapon/Armor/Actor p1 are DB refs.

### 1.4 Inventory / party / actor / enemy (DB-id reference sites)

| code | name | parameters[] (refs in **bold**) |
|---|---|---|
| 125 | Change Gold | `[0]`=op(0 inc/1 dec), `[1]`=operandType(0 const/1 var), `[2]`=value/varId |
| 126 | Change Items | `[0]`=**itemId→Items**, `[1]`=op, `[2]`=operandType, `[3]`=value/varId |
| 127 | Change Weapons | `[0]`=**weaponId→Weapons**, `[1]`=op, `[2]`=operandType, `[3]`=value/varId, `[4]`=includeEquip |
| 128 | Change Armors | `[0]`=**armorId→Armors**, `[1]`=op, `[2]`=operandType, `[3]`=value/varId, `[4]`=includeEquip |
| 129 | Change Party Member | `[0]`=**actorId→Actors**, `[1]`=op(0 add/1 remove), `[2]`=initialize |
| 303 | Name Input | `[0]`=**actorId→Actors**, `[1]`=maxChars |
| 311 | Change HP | `[0]`=desig(0 fixed/1 variable), `[1]`=**actorId→Actors** or varId, `[2]`=op, `[3]`=operandType, `[4]`=value/varId, `[5]`=allowDeath |
| 312 | Change MP | `[0]`/`[1]` actor target; `[2]`=op,`[3]`=operandType,`[4]`=value/varId |
| 313 | Change State | `[0]`/`[1]` actor target; `[2]`=op(0 add/1 remove), `[3]`=**stateId→States** |
| 314 | Recover All | `[0]`/`[1]` actor target (no value params) |
| 315 | Change EXP | `[0]`/`[1]` actor target; `[2]`=op,`[3]`=operandType,`[4]`=value/varId,`[5]`=showLevelUp |
| 316 | Change Level | `[0]`/`[1]` actor target; `[2]`=op,`[3]`=operandType,`[4]`=value/varId,`[5]`=showLevelUp |
| 317 | Change Parameter | `[0]`/`[1]` actor target; **`[2]`=paramId(0 mhp..7 luk)**; `[3]`=op,`[4]`=operandType,`[5]`=value/varId |
| 318 | Change Skill | `[0]`/`[1]` actor target; `[2]`=op(0 learn/1 forget), `[3]`=**skillId→Skills** |
| 319 | Change Equipment | `[0]`=**actorId→Actors** (direct), `[1]`=equip slot, `[2]`=**itemId** (Weapon/Armor by slot; 0=unequip) |
| 320 | Change Name | `[0]`=**actorId→Actors**, `[1]`=name string |
| 321 | Change Class | `[0]`=**actorId→Actors**, `[1]`=**classId→Classes**, `[2]`=keepExp |
| 322 | Change Actor Images | `[0]`=**actorId→Actors**, `[1]`=characterName(img/characters), `[2]`=charIndex, `[3]`=faceName(img/faces), `[4]`=faceIndex, `[5]`=battlerName(img/sv_actors) — **3 asset refs** |
| 323 | Change Vehicle Image | `[0]`=vehicleType, `[1]`=characterName(img/characters), `[2]`=charIndex |
| 324 | Change Nickname | `[0]`=**actorId→Actors**, `[1]`=nickname |
| 325 | Change Profile | `[0]`=**actorId→Actors**, `[1]`=profile |
| 326 | Change TP | `[0]`/`[1]` actor target; `[2]`=op,`[3]`=operandType,`[4]`=value/varId |
| 331 | Change Enemy HP | `[0]`=troop member index(-1 all, plain index — NOT a var); `[1]`=op,`[2]`=operandType,`[3]`=value/varId,`[4]`=allowDeath. `operateValue([1],[2],[3])` |
| 332 | Change Enemy MP | `[0]`=index; `[1]`=op,`[2]`=operandType,`[3]`=value/varId |
| 342 | Change Enemy TP | `[0]`=index; `[1]`=op,`[2]`=operandType,`[3]`=value/varId |
| 333 | Change Enemy State | `[0]`=troop member index(-1 all), `[1]`=op(0 add/1 remove), `[2]`=**stateId→States** |
| 336 | Enemy Transform | `[0]`=index, `[1]`=**enemyId→Enemies** |
| 337 | Show Battle Animation | `[0]`=index, `[1]`=**animationId→Animations**, `[2]`=forAll |

**Actor-target convention (311–318, 326) — `iterateActorEx([0],[1])`:** `[0]==0` → `[1]` is a **literal actorId**
(`0` = whole party, NOT a dangling ref); `[0]==1` → `[1]` is a **variableId (READ)** whose value is the actorId
(actor not statically known). Single-actor commands 319–325 take the actorId directly in `[0]`.

### 1.5 Transfer / battle / shop / movement (graph + map refs)

| code | name | parameters[] |
|---|---|---|
| 201 | Transfer Player | `[0]`=designation(0 direct/1 variable). If 0: `[1]`=**mapId→MapInfos/MapXXX**, `[2]`=x, `[3]`=y. If 1: `[1]/[2]/[3]` are **variableIds (READ; map not static)**. `[4]`=direction(0 retain/2/4/6/8), `[5]`=fadeType(0 black/1 white/2 none) |
| 202 | Set Vehicle Location | `[0]`=vehicleType, `[1]`=designation(0 direct/1 var), `[2]`=mapId or varId, `[3]/[4]`=x/y or varIds |
| 203 | Set Event Location | `[0]`=eventId (this map), `[1]`=type(0 direct/1 var/2 swap), `[2]/[3]`=x/y or varIds or other eventId, `[4]`=direction |
| 205 | Set Movement Route | `[0]`=characterId(-1 player/0 this event/>0 eventId), `[1]`=moveRoute object |
| 212 | Show Animation | `[0]`=characterId, `[1]`=**animationId→Animations**, `[2]`=wait |
| 301 | Battle Processing | `[0]`=designation(0 direct/1 variable/2 same-as-random-encounter). If 0: `[1]`=**troopId→Troops**; if 1: `[1]`=variableId (READ); if 2: `[1]` ignored. `[2]`=canEscape, `[3]`=canLose. Result arms 601 If Win/602 If Escape/603 If Lose |
| 302 | Shop Processing | first goods row in `parameters`, extra rows as code **605**. Row layout: `[0]`=type(0 item/1 weapon/2 armor), `[1]`=**dataId→Items/Weapons/Armors**, `[2]`=priceType, `[3]`=price, `[4]`=purchaseOnly |

### 1.6 Picture / media / map visuals (asset sites)

| code | name | asset param |
|---|---|---|
| 231 | Show Picture | `[0]`=pictureId, **`[1]`=picture name → img/pictures/** (""=none), `[2]`=origin(0 UL/1 center), `[3]`=designation(0 direct/1 var), `[4]/[5]`=x/y or varIds, `[6]`=scaleX%, `[7]`=scaleY%, `[8]`=opacity(0..255), `[9]`=blendMode |
| 232 | Move Picture | references existing pictureId; **MZ** adds `[10]/[11]/[12]` easing; no asset name |
| 233/234/235 | Rotate/Tint/Erase Picture | pictureId only, no asset |
| 241 | Play BGM | **`[0]`=audio object `{name,volume,pitch,pan}` → audio/bgm/** (`.name`; ""=stop) |
| 245 | Play BGS | **`[0]`.name → audio/bgs/** |
| 249 | Play ME | **`[0]`.name → audio/me/** |
| 250 | Play SE | **`[0]`.name → audio/se/** |
| 261 | Play Movie | **`[0]`=movie name → movies/** (ext `.webm` or `.mp4`) |
| 282 | **Change Tileset** | `[0]`=**tilesetId→Tilesets** (indirect → `tilesetNames[]` → img/tilesets/). **NOT battleback** (corrects earlier source) |
| 283 | **Change Battle Back** | `[0]`=**battleback1 name → img/battlebacks1/**, `[1]`=**battleback2 name → img/battlebacks2/** |
| 284 | Change Parallax | `[0]`=**parallax name → img/parallaxes/**, `[1..4]`=loop/sx/sy flags |

> **Resolved contradiction:** an early source mislabeled 282 as battleback and 283 as parallax. Two independent
> agents reading both core scripts confirm **282=Tileset, 283=BattleBack, 284=Parallax**, identical MV/MZ. Use these.

### 1.7 Scripts / plugin commands (opaque — route to AST/plugin layer)

| code | name | parameters[] |
|---|---|---|
| 355 / 655 | Script | `[0]`=one JS line; 355=first, consecutive 655=continuation; concatenated with `\n` and `eval`'d. Feed whole block to AST. |
| 356 | Plugin Command (**MV**) | `[0]`=ONE raw string. Engine: `args=[0].split(" "); command=args.shift()`. No registry — dispatched via overridden `Game_Interpreter.prototype.pluginCommand`. Only string-match first token; confidence `likely`. |
| 357 | Plugin Command (**MZ ONLY**, absent in MV) | `[0]`=pluginName(=js/plugins/<name>.js filename), `[1]`=commandName, `[2]`=editor display label (**UNUSED at runtime, skipped**), **`[3]`=args object `{argName:stringValue}` (all values strings even when numeric)**. Engine: `PluginManager.callCommand(this, [0], [1], [3])`; silent no-op if key `pluginName:commandName` not registered. |

> **Resolved contradiction:** the MZ 357 args object is `parameters[3]`, NOT `[2]`. `[2]` is the human-readable
> label the engine never reads. Confirmed by three agents against the core method body and a real on-disk sample.

---

## 2. Switch / variable READ-site & WRITE-site catalogue

Complete enumeration of every place a switch or variable is read or written (the symbol-table sites).

### 2.1 Switch sites

| Site | Kind | Where |
|---|---|---|
| `System.switches[id]` (name array) | DECLARATION | length defines valid ID range; name = display only |
| 121 Control Switches `[0]..[1]` | WRITE (range) | event/common/troop command lists |
| 123 Control Self Switch | WRITE (self-switch `[mapId,eventId,ch]`) | distinct namespace from global switches |
| 111 type 0 `[1]` | READ | conditional branch |
| 111 type 2 `[1]` | READ (self-switch) | |
| page `conditions.switch1Id` (if `switch1Valid`) | READ | MapXXX event page |
| page `conditions.switch2Id` (if `switch2Valid`) | READ | MapXXX event page |
| Troop `pages[].conditions.switchId` (if `switchValid`) | READ | Troops.json |
| CommonEvent `switchId` (when `trigger !== 0`) | READ | CommonEvents.json (autorun/parallel gate) |
| Enemy `actions[].conditionParam1` when `conditionType==6` | READ | Enemies.json |
| Plugin `@type switch` / `switch[]` param value | DECLARED/owned (Layer A, deferred) | plugins.js — IDs the plugin reads/writes at runtime; suppress "uninit" findings for these |

### 2.2 Variable sites

| Site | Kind | Where |
|---|---|---|
| `System.variables[id]` (name array) | DECLARATION | length defines valid ID range |
| 122 Control Variables `[0]..[1]` | WRITE (range) | |
| 103 Input Number `[0]` | WRITE | |
| 104 Select Item `[0]` | WRITE | |
| 111 type 1 `[1]`, and `[3]` if `[2]==1` | READ | conditional branch |
| 122 operand type 1 `[4]` (srcVarId) | READ | |
| 122 operand type 3 (gameData) actor/enemy stat selectors | READ (game-state, not a free var) | |
| 201 `[1]/[2]/[3]` when `[0]==1` | READ | transfer by variable |
| 202 / 203 when designation==1 | READ | vehicle/event location by variable |
| 301 `[1]` when `[0]==1` | READ | battle by variable |
| 311–318/326 `[1]` when `[0]==1` | READ (holds actorId) | |
| any `operandType==1` value slot (125/126/127/128/311…) | READ | |
| page `conditions.variableId` (if `variableValid`, with `variableValue`) | READ | MapXXX event page |
| Plugin `@type variable` / `variable[]` param value | DECLARED/owned (Layer A, deferred) | |

> `122 operand type 4` and `111 type 12` carry raw JS → variables/switches referenced inside are **opaque**
> (route to AST). Do not treat as no-reference.

---

## 3. Referential-integrity catalogue (every ID → file reference)

### 3.1 Database cross-file references

| Source field / command | → Target |
|---|---|
| `Actors[].classId` | Classes |
| `Actors[].equips[]` (slot 0 → Weapons, others → Armors; 0=empty) | Weapons / Armors |
| `Classes[].learnings[].skillId` | Skills |
| `Skills[].stypeId` | System.skillTypes |
| `Skills[].requiredWtypeId1/2` | System.weaponTypes |
| `Skills[].animationId` (-1 normal attack, 0 none) | Animations |
| `Skills[].damage.elementId` (-1 normal, 0 none) | System.elements |
| `Items[].animationId` | Animations |
| `Items[].damage.elementId` | System.elements |
| `Weapons[].wtypeId` | System.weaponTypes |
| `Weapons[].etypeId` | System.equipTypes |
| `Weapons[].animationId` | Animations |
| `Armors[].atypeId` | System.armorTypes |
| `Armors[].etypeId` | System.equipTypes |
| `Enemies[].actions[].skillId` | Skills |
| `Enemies[].actions[].conditionParam1` when `conditionType==4` | States |
| `Enemies[].actions[].conditionParam1` when `conditionType==6` | System.switches |
| `Enemies[].dropItems[]` `{kind,dataId}`: kind 1→Items, 2→Weapons, 3→Armors | Items/Weapons/Armors |
| `Troops[].members[].enemyId` | Enemies |
| `Troops[].pages[].conditions.actorId` (if `actorValid`) | Actors |
| MapInfo `parentId` (0=root) | another MapInfos entry |
| `System.startMapId`, `editMapId`, `boat/ship/airship.startMapId` | a Map |
| `System.partyMembers[]`, `testBattlers[].actorId` | Actors |
| `System.testTroopId` | Troops |
| Skill/Item `effects[].code` 21/22 → States, 43 → Skills, 44 → CommonEvents | (see §3.3) |
| `traits[].code`/`dataId` typed FK | (see §3.2) |

### 3.2 TRAIT `code` → `dataId` target (`traits[]` in Actors/Classes/Enemies/Weapons/Armors/States)

| code | name | dataId → |
|---|---|---|
| 11 ELEMENT_RATE / 31 ATTACK_ELEMENT | System.elements |
| 13 STATE_RATE / 14 STATE_RESIST / 32 ATTACK_STATE | States |
| 35 ATTACK_SKILL / 43 SKILL_ADD / 44 SKILL_SEAL | Skills |
| 41 STYPE_ADD / 42 STYPE_SEAL | System.skillTypes |
| 51 EQUIP_WTYPE | System.weaponTypes |
| 52 EQUIP_ATYPE | System.armorTypes |
| 53 EQUIP_LOCK / 54 EQUIP_SEAL | System.equipTypes |
| 12/21 (param idx), 22/23 (ex/sp-param idx), 33/34/55/61/62/63/64 | value/enum only — **NOT** referential targets |

### 3.3 EFFECT `code` → `dataId` target (`effects[]` in Skills/Items)

| code | name | dataId → |
|---|---|---|
| 21 ADD_STATE (dataId 0 = "Normal Attack" state set) | States |
| 22 REMOVE_STATE | States |
| 43 LEARN_SKILL | Skills |
| 44 COMMON_EVENT | CommonEvents |
| 11/12/13 recover, 31/32/33/34 buff (param idx), 41 special, 42 grow (param idx) | value/enum only — NOT targets |

### 3.4 Event-command DB references (re-stated from §1 for the integrity rule)

`117→CommonEvents`; `201 [1] (designation 0)→Map`; `126→Items`; `127→Weapons`; `128→Armors`;
`129/303/311–326 actorId→Actors`; `212 [1]→Animations`; `301 [1] (designation 0)→Troops`;
`302/605 [1]→Items/Weapons/Armors`; `313/333 stateId→States`; `318 skillId→Skills`; `321 classId→Classes`;
`336 enemyId→Enemies`; `111 type 4–10 [1]/[3]→Actors/Skills/Weapons/Armors/Items/States`.

---

## 4. Asset-reference catalogue (field/command → folder + extension)

**Resolver model:** an asset reference is a **bare name, no folder, no extension**. `diskPath = folder + name + ext`
(+ encryption suffix). Empty string `""` = "no asset" → skip, never flag. `*Index` fields (characterIndex,
faceIndex) are sprite-sheet cell positions, **not** files.

### 4.1 Folder + extension table

| Folder | Ext (MV) | Ext (MZ) | Loader |
|---|---|---|---|
| img/characters/ | .png | .png | loadCharacter |
| img/faces/ | .png | .png | loadFace |
| img/pictures/ | .png | .png | loadPicture |
| img/parallaxes/ | .png | .png | loadParallax |
| img/tilesets/ | .png | .png | loadTileset |
| img/battlebacks1/ , img/battlebacks2/ | .png | .png | loadBattleback1/2 |
| img/enemies/ , img/sv_enemies/ | .png | .png | loadEnemy / loadSvEnemy |
| img/sv_actors/ | .png | .png | loadSvActor |
| img/titles1/ , img/titles2/ | .png | .png | loadTitle1/2 |
| img/system/ | .png | .png | loadSystem (engine-required set; never orphan) |
| img/animations/ | .png | .png (MV-style entries only) | loadAnimation |
| **effects/** (TOP-LEVEL, MZ) | — | **.efkefc** | EffectManager.makeUrl (Effekseer) |
| audio/bgm , bgs , me , se | .ogg **and** .m4a | .ogg | AudioManager |
| movies/ | .webm / .mp4 | .webm / .mp4 | command 261 |

> MV ships **both** `.ogg` and `.m4a` per sound → a sound is "present" if either exists; treat the pair as one
> logical asset for orphan counting. MZ is `.ogg` only.

### 4.2 Data-file asset fields

| File.field | Folder |
|---|---|
| Actors `characterName` / `faceName` / `battlerName` | characters / faces / **sv_actors** |
| Enemies `battlerName` | **img/enemies/** (frontview) OR **img/sv_enemies/** (sideview; mode = `System.optSideView`) — accept either unless the flag is read |
| Tilesets `tilesetNames[9]` (slots A1,A2,A3,A4,A5,B,C,D,E; ""=unused) | tilesets |
| Animations (MV-style, `!!frames`) `animation1Name`/`animation2Name`; `timings[].se` | img/animations/ ; audio/se/ |
| Animations (MZ Effekseer, no `frames`) `effectName`; `soundTimings[].se` | effects/*.efkefc ; audio/se/ |
| Map `battleback1Name`/`battleback2Name`/`parallaxName`; `bgm`/`bgs` autoplay objects | battlebacks1/2 ; parallaxes ; bgm/bgs |
| System `title1Name`/`title2Name` | titles1/2 |
| System `battleback1Name`/`battleback2Name` (battle test) | battlebacks1/2 |
| System `boat/ship/airship.characterName` | characters |
| System `sounds[24]` (SE objects), `titleBgm`,`battleBgm` (bgm), `victoryMe`/`defeatMe`/`gameoverMe` (me), `boat/ship/airship.bgm` | se / bgm / me |
| System `advanced.numberFontFilename` / `mainFontFilename` (MZ/modern MV) | fonts/ |

All audio refs are objects `{name,volume,pitch,pan}` — only `.name` is the asset; ""=silence, skip.

### 4.3 Command asset references (from §1.6)

`101 [0]`→faces; `231 [1]`→pictures; `241/245/249/250 [0].name`→bgm/bgs/me/se; `261 [0]`→movies;
`282 [0]`→tilesetId→tilesets (indirect); `283 [0]/[1]`→battlebacks1/2; `284 [0]`→parallaxes;
`322 [1]/[3]/[5]`→characters/faces/sv_actors; `323 [1]`→characters; `212 [1]`→animationId→Animations (indirect).

### 4.4 Encryption (resolver must accept encrypted variants)

Driven by `System.json` `hasEncryptedImages` / `hasEncryptedAudio` / `encryptionKey` (hex).

- **MV** (different extension): `.png→.rpgmvp`, `.ogg→.rpgmvo`, `.m4a→.rpgmvm`. `img/system/Window.png` is in
  `Decrypter._ignoreList` (never encrypted).
- **MZ** (appends literal `_` to the whole filename): `Actor1.png` on disk → `Actor1.png_`; `Town.ogg` → `Town.ogg_`.
- Resolver: when the relevant flag is set, accept plain OR encrypted-suffix variant; broken only if **none** exist.
  When flags false / key empty, only plain extensions valid. (MZ effekseer `_` suffix handling unverified — open Q.)

### 4.5 Orphan detection guards

Collect every referenced `(folder,name)`; normalize disk files by stripping known ext + encryption suffix; any
file under a known asset folder whose stem is unreferenced is a candidate orphan. Guards: empty-string names valid;
`img/system/` default set never orphan; MV ogg+m4a pair = one asset; leading `$`/`!` in names are real filename
chars (single-sheet / no-shift flags) — keep them; tileset/`sounds[]` empty slots are intentional; non-`.efkefc`
files under `effects/` are transitively referenced by `.efkefc` → treat as alive; plugin-referenced assets
(`@type file`, command args, notetags) must be merged before declaring orphans → flag as "possibly used", not orphan.

---

## 5. Per-file shape reference (the analysis spine)

- **System.json** (single object): `switches[]`, `variables[]` (symbol table); type arrays `elements`,
  `skillTypes`, `weaponTypes`, `armorTypes`, `equipTypes` (1-based, idx0=""); `startMapId`, `partyMembers`,
  `testTroopId`, `testBattlers[]`; asset fields (§4.2); `hasEncryptedImages/Audio`, `encryptionKey`;
  `optSideView`. **MZ/modern-MV optional:** `advanced` (`gameId`, screen size, fonts; MZ adds `windowOpacity`),
  `battleSystem`, `itemCategories`, `optAutosave`, `optKeyItemsNumber`, `titleCommandWindow`, `tileSize`. Treat
  all of these as optional everywhere (old MV ≤1.4 lacks `advanced`).
- **MapInfos.json** (sparse array, idx=mapId): `{id,name,parentId,order,expanded,scrollX,scrollY}`. Authority for
  "which maps exist"; id N → `MapNNN.json` (3-digit zero-padded).
- **MapXXX.json** (object): `width,height,tilesetId,scrollType,data[]` (flat tile-id int array),
  `encounterList`, `bgm`/`bgs`, `battleback*Name`, `parallaxName`, `events[]` (sparse, null at 0).
  - **Event:** `{id,name,note,x,y,pages[]}`.
  - **Page:** `{conditions,image,list,trigger,moveRoute,moveType,moveSpeed,moveFrequency,directionFix,
    walkAnime,stepAnime,through,priorityType}`. `trigger`: 0 Action,1 Player Touch,2 Event Touch,3 Autorun,4 Parallel.
  - **Page `conditions`:** `switch1Valid/switch1Id`, `switch2Valid/switch2Id`, `variableValid/variableId/
    variableValue`, `selfSwitchValid/selfSwitchCh`, `itemValid/itemId`, `actorValid/actorId`. **An id is a real
    read only when its `*Valid` flag is true** (a default `switch1Id:1` with `switch1Valid:false` is NOT a read).
  - **Page `image`** (verified keys exactly): `characterName, characterIndex, tileId, direction, pattern`.
    **NO `faceName`/`faceIndex`** — faces come only from commands 101 and 322. If `tileId>0` event renders as a
    tile (no sprite); else uses `characterName` (""=invisible).
- **CommonEvents.json** (array): `{id,name,trigger,switchId,list}`. `trigger`: 0 None,1 Autorun,2 Parallel.
  `switchId` is a READ when `trigger!==0`. Reached via command 117 (graph edge).
- **Troops.json** (array): `{id,name,members[],pages[]}`. `members[]={enemyId,x,y,hidden}` (x/y screen px).
  `pages[]={conditions,list,span}`; `span`: 0 Battle(once),1 Turn,2 Moment. Page runs only if ≥1 of
  `turnEnding/turnValid/enemyValid/actorValid/switchValid` set (else never runs → "dead battle page" finding).
  `conditions`: `turnEnding`, `turnValid/turnA/turnB`, `enemyValid/enemyIndex/enemyHp`,
  `actorValid/actorId/actorHp`, `switchValid/switchId`.
- **Database tables** (Actors/Classes/Skills/Items/Weapons/Armors/Enemies/States/Animations/Tilesets): per §2–§3
  fields above. `iconIndex` (Skills/Items/Weapons/Armors/States) = index into shared `img/system/IconSet`,
  **not** a per-record asset filename.
- `_databaseFiles` ($dataXxx ↔ *.json) is **identical** MV↔MZ; no data file added/removed between engines.

### Command-list walking (indent invariant — identical MV/MZ)

Each command `{code,indent,parameters}`. A block's body = following commands whose `indent` is **strictly greater**
than the opener's (`skipBranch`: `while(list[index+1].indent > this._indent) index++` — byte-identical MV/MZ).
Codes with no `command<NNN>` method are silent no-ops: 401/405/408/655/605 (continuations), 404/412 (terminators),
402/403/601/602/603 (branch arms — treat like 411). `code:0` = blank line ending a block. `_branch[indent]` is
shared by If/choice/battle-result arms.

---

## 6. MV-vs-MZ delta (concise)

| Aspect | MV | MZ |
|---|---|---|
| Plugin command | **356** raw string `[0]`; no registry; dispatched via `pluginCommand` override | **357** struct (`[0]` plugin, `[1]` command, `[3]` args object); validated against `registerCommand`/`@command`; silent no-op if unregistered. (356 also exists in MZ as deprecated fallback) |
| Show Text 101 | len 4 (`[0..3]`) | len 5; `[4]`=speaker name |
| Animations | always MV-style: `animation1Name`/`animation2Name` (img/animations/), `timings[].se` | per-entry: MV-style iff `!!frames`, else Effekseer `effectName` (effects/*.efkefc), `soundTimings[]`/`flashTimings[]` |
| Encryption on disk | `.rpgmvp`/`.rpgmvo`/`.rpgmvm` (changed ext) | append `_` to full name (`Actor1.png_`, `Town.ogg_`) |
| Audio ext | ships `.ogg`+`.m4a` | `.ogg` only |
| 111 type 11 Button | `[1]` key only | adds `[2]` press mode |
| 232 Move Picture | no easing params | adds `[10]/[11]/[12]` easing |
| System.json | old MV (≤1.4) lacks `advanced`; 1.5+ has it | `advanced` + `windowOpacity`, `battleSystem`, `itemCategories`, `optAutosave`, etc. |
| Folder layout | often `www/` wrapper (data at `www/data`, js at `www/js`) | no `www/`; `data/`,`js/`,`img/`,`audio/`,`effects/` at root |
| Core scripts | per-class `rpg_*.js` (open mirror) / monolith `rpg_objects.js` (retail) | monolithic `rmmz_*.js` |
| Command index meaning | — | **identical to MV for all commands except 101 and the 356/357 split** |

Adapter must: try project root then fall back to `www/`; detect engine by presence of `command357` usage / `effects/`
folder / MZ-only System fields; branch Animations per-entry on `frames`; branch encryption suffix per engine.

---

## 7. plugins.js shape (capture for IR now; interpretation deferred to Layer A/B)

`js/plugins.js` is NOT JSON: `var $plugins = [ … ];` with a 2-line generated header. Strip the `var $plugins =`
prefix / trailing `;` (or regex-extract the array) before `JSON.parse`. Array order = load order.

Each entry: `{name, status, description, parameters}`. `name` = plugin filename without `.js`. `status:true` =
ENABLED (only enabled plugins register commands / contribute order constraints). `parameters` = `{paramName:string}`
— **every value is a string**; struct/array params are JSON-strings nested inside JSON-strings (2–4 escape levels),
decoded by cascaded `JSON.parse`. `PluginManager.parameters(name)` keys by `name.toLowerCase()`.

Header annotation block `/*: … */` (default; localized `/*:ja` etc.). Plugin-level tags: `@target MV|MZ`,
`@plugindesc`, `@author`, `@url`, `@help`, `@base <name>` (hard dep, must exist+enabled+load-before),
`@orderAfter <name>`, `@orderBefore <name>`. Param tags: `@param <name>` (storage key, NOT `@text`), `@text`,
`@desc`, `@default`, `@parent`, `@type`, `@min/@max/@decimals`, `@option/@value`, `@on/@off`, `@dir/@require`.
`@type` enum: `string,note,number,boolean,file,animation,select,combo,actor,class,skill,item,weapon,armor,enemy,
troop,state,tileset,common_event,switch,variable,struct<Name>` (+`[]` for arrays). Struct defs: `/*~struct~Name:`.
Command tags (MZ only): `@command <name>` (== registerCommand commandName == on-disk 357 `[1]`), `@arg <name>`
(== keys of 357 `[3]`).

**Layer-A hook (deferred):** every ID in a `@type switch`/`switch[]`/`variable`/`variable[]` param value (after
nested decode) is a switch/var the plugin owns and initializes at runtime → mark `declared_by_plugin` and **suppress
"uninitialized" findings** for it. `@command`/`@base`/`@orderAfter`/`@orderBefore` feed the future plugin-registry
and load-order rules; validated against the actual `$plugins` array order.
