//! Tier B: AST heuristics over the JS code of plugins and event scripts (via the
//! `ress` lexer).
//!
//! This is engine-specific (RPG Maker) parsing — it lives in the adapter, the core
//! stays agnostic. We parse **only** ENABLED plugins and script blackboxes (355/655,
//! 111 type 12, 122 operand 4). Tolerance: the lexer aborts on error, and any panic
//! is isolated by `catch_unwind` — an unreadable file simply yields fewer facts, it
//! does not crash the analysis.
//!
//! We extract (all at `likely` confidence):
//!  - literal switch/var writes (`$gameSwitches/$gameVariables.setValue(N, …)`);
//!    for self-switches we resolve only the CURRENT-EVENT idiom
//!    (`$gameSelfSwitches.setValue/value([this._mapId, this._eventId, 'A'], …)`),
//!    binding the channel to the script's own event — foreign/computed keys stay opaque;
//!  - command registration (`PluginManager.registerCommand(plugin, command, …)`)
//!    with name resolution via simple constant propagation;
//!  - core-method patches (`X.prototype.m = …`) with alias/overwrite classification.

use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Simplified token — exactly as much as the shallow patterns need.
#[derive(Clone, Debug, PartialEq)]
enum Tok {
    /// Identifier.
    Id(String),
    /// String literal (contents without quotes).
    Str(String),
    /// Numeric literal parsed into `u64` (or `None` if not an integer).
    Num(Option<u64>),
    /// `.`
    Dot,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `,`
    Comma,
    /// `=` (assignment; `==`/`===`/`=>` are emitted by the lexer as separate punctuators).
    Eq,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// Other (any other token; serves as a barrier for the patterns).
    Other,
}

/// Lexes the source into a stream of simplified tokens; comments are dropped,
/// strings/numbers retain their value. On a lexer error the traversal aborts.
fn lex(src: &str) -> Vec<Tok> {
    use ress::prelude::*;
    let mut out = Vec::new();
    for item in Scanner::new(src) {
        let Ok(item) = item else { break };
        match item.token {
            Token::Ident(i) => out.push(Tok::Id(i.to_string())),
            Token::String(s) => out.push(Tok::Str(match s {
                StringLit::Single(x) | StringLit::Double(x) => x.content.to_string(),
            })),
            Token::Number(n) => out.push(Tok::Num(n.to_string().trim().parse::<u64>().ok())),
            Token::Punct(p) => out.push(match p {
                Punct::Period => Tok::Dot,
                Punct::OpenParen => Tok::LParen,
                Punct::CloseParen => Tok::RParen,
                Punct::Comma => Tok::Comma,
                Punct::Equal => Tok::Eq,
                Punct::OpenBracket => Tok::LBracket,
                Punct::CloseBracket => Tok::RBracket,
                _ => Tok::Other,
            }),
            // `this` is a keyword, not an Ident — keep it as an Id so the
            // current-event self-switch idiom (`this._mapId`/`this._eventId`) matches.
            // Other keywords stay opaque barriers.
            Token::Keyword(k) => out.push(if k.as_str() == "this" {
                Tok::Id("this".to_string())
            } else {
                Tok::Other
            }),
            Token::Comment(_) => {}
            Token::EoF => break,
            _ => out.push(Tok::Other),
        }
    }
    out
}

/// Lexes the source, isolating any lexer panic (returns empty).
fn lex_safe(src: &str) -> Vec<Tok> {
    catch_unwind(AssertUnwindSafe(|| lex(src))).unwrap_or_default()
}

/// Builds a map of simple constants `IDENT = "literal"` (const/let/var and
/// reassignments) — for resolving the plugin name in `registerCommand`.
fn const_map(t: &[Tok]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for i in 0..t.len() {
        if let (Tok::Id(name), Some(Tok::Eq), Some(Tok::Str(val))) =
            (&t[i], t.get(i + 1), t.get(i + 2))
        {
            map.entry(name.clone()).or_insert_with(|| val.clone());
        }
    }
    map
}

/// `true` if the token is an identifier with the given name (without allocation).
fn is_id(tok: Option<&Tok>, name: &str) -> bool {
    matches!(tok, Some(Tok::Id(s)) if s == name)
}

/// Self-switch channel (`'A'..'D'`) from a single-character string literal.
fn self_switch_channel(s: &str) -> Option<char> {
    let mut ch = s.chars();
    let c = ch.next()?;
    if ch.next().is_none() && matches!(c, 'A'..='D') {
        Some(c)
    } else {
        None
    }
}

/// Facts extracted from the JS of an enabled plugin.
#[derive(Debug, Default)]
pub struct PluginJsFacts {
    /// Literal ids of switches written by the plugin (`setValue`).
    pub switch_writes: Vec<u32>,
    /// Literal ids of variables written by the plugin (`setValue`).
    pub variable_writes: Vec<u32>,
    /// Literal ids of variables read by the plugin (`$gameVariables.value(N)`).
    pub variable_reads: Vec<u32>,
    /// Registered commands `(plugin-name, command-name)` with resolved
    /// (literal/constant) arguments.
    pub commands: Vec<(String, String)>,
    /// `true` if at least one `registerCommand` was found.
    pub registers_any: bool,
    /// `true` if ALL `registerCommand` calls had resolvable arguments (the registry
    /// can be considered complete). `false` on any dynamic/untracked
    /// registration.
    pub registry_complete: bool,
    /// Core-method patches `(method, whether_it_overwrites)`.
    pub patches: Vec<(String, bool)>,
    /// Literal ids of common events reserved via
    /// `$gameTemp.reserveCommonEvent(N)` (saves them from `dead-common-event`).
    pub reserved_common_events: Vec<u32>,
}

/// Facts extracted from an event-script blackbox (355/655, 111-12, 122-4).
#[derive(Debug, Default)]
pub struct ScriptJsFacts {
    /// Literal ids of written switches.
    pub switch_writes: Vec<u32>,
    /// Literal ids of written variables.
    pub variable_writes: Vec<u32>,
    /// Literal ids of variables read (`$gameVariables.value(N)`).
    pub variable_reads: Vec<u32>,
    /// Self-switch channels WRITTEN via the current-event idiom
    /// (`$gameSelfSwitches.setValue([this._mapId, this._eventId, 'X'], …)`).
    pub self_switch_writes: Vec<char>,
    /// Self-switch channels READ via the current-event idiom
    /// (`$gameSelfSwitches.value([this._mapId, this._eventId, 'X'])`).
    pub self_switch_reads: Vec<char>,
    /// Literal ids of common events reserved via
    /// `$gameTemp.reserveCommonEvent(N)` in this event script.
    pub reserved_common_events: Vec<u32>,
}

/// **Curated** list of prototype-bearing core classes of RPG Maker MV/MZ.
///
/// Conflicts are valuable precisely on these classes. Prefix matching (`Window_*`)
/// does NOT work here: plugins name their own classes by the same convention
/// (`Window_SrpgPrediction`, `Sprite_SrpgMoveTile`, `Game_SrpgUnit`), and an author
/// patching their own class / its addon is not a conflict. An exact list rules out
/// plugin classes and built-in types (`Array`/`Object` — polyfills). Managers
/// (`DataManager` etc.) are static (no prototype) and not needed here. Missing
/// some rare class only lowers recall (fewer findings), without creating
/// false ones — an acceptable trade-off in favor of precision.
const ENGINE_CLASSES: &[&str] = &[
    // Game_*
    "Game_Temp",
    "Game_System",
    "Game_Screen",
    "Game_Picture",
    "Game_Timer",
    "Game_Message",
    "Game_Switches",
    "Game_Variables",
    "Game_SelfSwitches",
    "Game_Actors",
    "Game_Party",
    "Game_Troop",
    "Game_Map",
    "Game_CommonEvent",
    "Game_CharacterBase",
    "Game_Character",
    "Game_Player",
    "Game_Follower",
    "Game_Followers",
    "Game_Vehicle",
    "Game_Event",
    "Game_Interpreter",
    "Game_Action",
    "Game_ActionResult",
    "Game_BattlerBase",
    "Game_Battler",
    "Game_Actor",
    "Game_Enemy",
    "Game_Unit",
    "Game_Enemies",
    // Scene_*
    "Scene_Base",
    "Scene_Boot",
    "Scene_Title",
    "Scene_Map",
    "Scene_MenuBase",
    "Scene_Menu",
    "Scene_ItemBase",
    "Scene_Item",
    "Scene_Skill",
    "Scene_Equip",
    "Scene_Status",
    "Scene_Options",
    "Scene_File",
    "Scene_Save",
    "Scene_Load",
    "Scene_GameEnd",
    "Scene_Shop",
    "Scene_Name",
    "Scene_Debug",
    "Scene_Battle",
    "Scene_Gameover",
    "Scene_Message",
    "Scene_BattleUI",
    // Window_*
    "Window_Base",
    "Window_Scrollable",
    "Window_Selectable",
    "Window_Command",
    "Window_HorzCommand",
    "Window_Help",
    "Window_Gold",
    "Window_StatusBase",
    "Window_MenuCommand",
    "Window_MenuStatus",
    "Window_MenuActor",
    "Window_ItemCategory",
    "Window_ItemList",
    "Window_SkillType",
    "Window_SkillStatus",
    "Window_SkillList",
    "Window_EquipStatus",
    "Window_EquipCommand",
    "Window_EquipSlot",
    "Window_EquipItem",
    "Window_Status",
    "Window_Options",
    "Window_SavefileList",
    "Window_ShopCommand",
    "Window_ShopBuy",
    "Window_ShopSell",
    "Window_ShopNumber",
    "Window_ShopStatus",
    "Window_NameEdit",
    "Window_NameInput",
    "Window_NameBox",
    "Window_ChoiceList",
    "Window_NumberInput",
    "Window_EventItem",
    "Window_Message",
    "Window_ScrollText",
    "Window_MapName",
    "Window_BattleLog",
    "Window_PartyCommand",
    "Window_ActorCommand",
    "Window_BattleStatus",
    "Window_BattleActor",
    "Window_BattleEnemy",
    "Window_BattleSkill",
    "Window_BattleItem",
    "Window_TitleCommand",
    "Window_GameEnd",
    "Window_DebugRange",
    "Window_DebugEdit",
    // Sprite_* / Spriteset_*
    "Sprite_Base",
    "Sprite_Clickable",
    "Sprite_Button",
    "Sprite_Character",
    "Sprite_Battler",
    "Sprite_Actor",
    "Sprite_Enemy",
    "Sprite_Animation",
    "Sprite_AnimationMV",
    "Sprite_Damage",
    "Sprite_Gauge",
    "Sprite_Name",
    "Sprite_StateIcon",
    "Sprite_StateOverlay",
    "Sprite_Weapon",
    "Sprite_Balloon",
    "Sprite_Picture",
    "Sprite_Timer",
    "Sprite_Destination",
    "Spriteset_Base",
    "Spriteset_Map",
    "Spriteset_Battle",
    // Base graphics / runtime core
    "Bitmap",
    "Sprite",
    "Tilemap",
    "TilingSprite",
    "ScreenSprite",
    "Window",
    "WindowLayer",
    "Weather",
    "Stage",
    "Scene_Manager",
];

/// Whether the class is in the curated core list ([`ENGINE_CLASSES`]).
fn is_core_class(class: &str) -> bool {
    ENGINE_CLASSES.contains(&class)
}

/// The `Class.prototype.method` chain for a **core class**, if the `prototype` token
/// at position `i` is part of it. Returns `(method, method_token_index)`.
/// Patches of non-core classes are filtered out here (see [`is_core_class`]).
fn prototype_method(t: &[Tok], i: usize) -> Option<(String, usize)> {
    if i < 2 {
        return None;
    }
    let Tok::Id(p) = &t[i] else { return None };
    if p != "prototype" || t[i - 1] != Tok::Dot || t.get(i + 1) != Some(&Tok::Dot) {
        return None;
    }
    let (Tok::Id(cls), Some(Tok::Id(m))) = (&t[i - 2], t.get(i + 2)) else {
        return None;
    };
    if !is_core_class(cls) {
        return None;
    }
    Some((format!("{cls}.prototype.{m}"), i + 2))
}

/// Scans literal global switch/var writes (`$gameSwitches/$gameVariables.setValue(N, …)`).
fn scan_setvalue(t: &[Tok], switches: &mut Vec<u32>, variables: &mut Vec<u32>) {
    for i in 0..t.len() {
        let Tok::Id(obj) = &t[i] else { continue };
        if t.get(i + 1) != Some(&Tok::Dot)
            || !is_id(t.get(i + 2), "setValue")
            || t.get(i + 3) != Some(&Tok::LParen)
        {
            continue;
        }
        let target = match obj.as_str() {
            "$gameSwitches" => &mut *switches,
            "$gameVariables" => &mut *variables,
            _ => continue,
        };
        if let Some(Tok::Num(Some(n))) = t.get(i + 4)
            && *n != 0
            && *n <= u32::MAX as u64
        {
            target.push(*n as u32);
        }
    }
}

/// Scans the CURRENT-EVENT self-switch idiom:
/// `$gameSelfSwitches.setValue([this._mapId, this._eventId, 'X'], …)` (write) and
/// `$gameSelfSwitches.value([this._mapId, this._eventId, 'X'])` (read). The key must
/// be exactly the script's own event so the channel binds to a known `(map, event)`;
/// a foreign or computed key is skipped to avoid cross-event misattribution.
fn scan_self_switch_current_event(t: &[Tok], writes: &mut Vec<char>, reads: &mut Vec<char>) {
    for i in 0..t.len() {
        if !is_id(t.get(i), "$gameSelfSwitches") || t.get(i + 1) != Some(&Tok::Dot) {
            continue;
        }
        let is_write = match t.get(i + 2) {
            Some(Tok::Id(s)) if s == "setValue" => true,
            Some(Tok::Id(s)) if s == "value" => false,
            _ => continue,
        };
        if t.get(i + 3) != Some(&Tok::LParen) {
            continue;
        }
        if let Some(ch) = current_event_channel(t, i + 4) {
            if is_write {
                writes.push(ch);
            } else {
                reads.push(ch);
            }
        }
    }
}

/// Matches exactly `[this._mapId, this._eventId, '<ch>']` starting at the `[` token
/// (`lb`), returning the self-switch channel. Any other key shape yields `None`.
fn current_event_channel(t: &[Tok], lb: usize) -> Option<char> {
    if t.get(lb) != Some(&Tok::LBracket)
        || !is_id(t.get(lb + 1), "this")
        || t.get(lb + 2) != Some(&Tok::Dot)
        || !is_id(t.get(lb + 3), "_mapId")
        || t.get(lb + 4) != Some(&Tok::Comma)
        || !is_id(t.get(lb + 5), "this")
        || t.get(lb + 6) != Some(&Tok::Dot)
        || !is_id(t.get(lb + 7), "_eventId")
        || t.get(lb + 8) != Some(&Tok::Comma)
        || t.get(lb + 10) != Some(&Tok::RBracket)
    {
        return None;
    }
    let Some(Tok::Str(s)) = t.get(lb + 9) else {
        return None;
    };
    self_switch_channel(s)
}

/// Scans literal variable reads `$gameVariables.value(N)` (N a positive literal).
/// The read accessor `value(N)` is how plugins/scripts consume a game variable;
/// a variable read only this way is not dead even if written from data.
fn scan_value_reads(t: &[Tok], variables: &mut Vec<u32>) {
    for i in 0..t.len() {
        if is_id(t.get(i), "$gameVariables")
            && t.get(i + 1) == Some(&Tok::Dot)
            && is_id(t.get(i + 2), "value")
            && t.get(i + 3) == Some(&Tok::LParen)
            && let Some(Tok::Num(Some(n))) = t.get(i + 4)
            && *n != 0
            && *n <= u32::MAX as u64
        {
            variables.push(*n as u32);
        }
    }
}

/// Scans literal `<obj>.reserveCommonEvent(N)` calls (reserving a common
/// event). The dot before the name guarantees this is a method access, not a
/// same-named identifier. The method definition
/// `Game_Temp.prototype.reserveCommonEvent = function(id)` is ruled out precisely
/// by the `( + numeric literal` requirement right after the name: there the next
/// token is `=`, not `(`, so it never reaches the argument check.
fn scan_reserve_common_event(t: &[Tok], out: &mut Vec<u32>) {
    for i in 1..t.len() {
        if is_id(t.get(i), "reserveCommonEvent")
            && t[i - 1] == Tok::Dot
            && t.get(i + 1) == Some(&Tok::LParen)
            && let Some(Tok::Num(Some(n))) = t.get(i + 2)
            && *n != 0
            && *n <= u32::MAX as u64
        {
            out.push(*n as u32);
        }
    }
}

/// Parses the JS of an enabled plugin into [`PluginJsFacts`] (tolerantly).
pub fn analyze_plugin(src: &str) -> PluginJsFacts {
    let t = lex_safe(src);
    let mut facts = PluginJsFacts {
        registry_complete: true,
        ..Default::default()
    };

    scan_setvalue(&t, &mut facts.switch_writes, &mut facts.variable_writes);
    scan_value_reads(&t, &mut facts.variable_reads);
    scan_reserve_common_event(&t, &mut facts.reserved_common_events);

    let consts = const_map(&t);
    // Resolves an argument to a string: a literal as-is, an identifier — via consts.
    let resolve = |tok: Option<&Tok>| -> Option<String> {
        match tok {
            Some(Tok::Str(s)) => Some(s.clone()),
            Some(Tok::Id(n)) => consts.get(n).cloned(),
            _ => None,
        }
    };

    // Saved method originals (alias): `IDENT = X.prototype.m`.
    let mut saved: std::collections::HashSet<String> = std::collections::HashSet::new();
    // Assigned methods: `X.prototype.m = …`.
    let mut assigned: Vec<String> = Vec::new();

    for i in 0..t.len() {
        // registerCommand: PluginManager.registerCommand(arg0, arg1, …).
        if let Tok::Id(obj) = &t[i]
            && obj == "PluginManager"
            && t.get(i + 1) == Some(&Tok::Dot)
            && is_id(t.get(i + 2), "registerCommand")
            && t.get(i + 3) == Some(&Tok::LParen)
        {
            facts.registers_any = true;
            // arg0 = i+4, comma = i+5, arg1 = i+6.
            let ok = t.get(i + 5) == Some(&Tok::Comma);
            match (
                ok.then(|| resolve(t.get(i + 4))).flatten(),
                ok.then(|| resolve(t.get(i + 6))).flatten(),
            ) {
                (Some(plugin), Some(command)) if !plugin.is_empty() && !command.is_empty() => {
                    facts.commands.push((plugin, command));
                }
                _ => facts.registry_complete = false,
            }
        }

        // Prototype patches.
        if let Some((method, m_idx)) = prototype_method(&t, i) {
            if t.get(m_idx + 1) == Some(&Tok::Eq) {
                // `X.prototype.m = …` — an assignment.
                assigned.push(method);
            } else if i >= 3 && t[i - 3] == Tok::Eq {
                // `IDENT = X.prototype.m` — saving the original (alias).
                saved.insert(method);
            }
        }
    }

    // Classify the assignments: an overwrite if the original is saved nowhere.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for method in assigned {
        if !seen.insert(method.clone()) {
            continue;
        }
        let overwrites = !saved.contains(&method);
        facts.patches.push((method, overwrites));
    }

    facts
}

/// Parses an event-script blackbox into [`ScriptJsFacts`] (tolerantly).
pub fn analyze_script(src: &str) -> ScriptJsFacts {
    let t = lex_safe(src);
    let mut facts = ScriptJsFacts::default();
    scan_setvalue(&t, &mut facts.switch_writes, &mut facts.variable_writes);
    scan_value_reads(&t, &mut facts.variable_reads);
    scan_self_switch_current_event(
        &t,
        &mut facts.self_switch_writes,
        &mut facts.self_switch_reads,
    );
    scan_reserve_common_event(&t, &mut facts.reserved_common_events);
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_literal_switch_var_writes_ignoring_strings_and_comments() {
        let src = r#"
            // $gameSwitches.setValue(99, true) in a comment
            const s = "$gameVariables.setValue(98, 1)";
            $gameSwitches.setValue(1, true);
            $gameVariables.setValue(7, 5);
            $gameSwitches.setValue($gameVariables.value(3), true); // computed -> skip
        "#;
        let f = analyze_plugin(src);
        assert_eq!(f.switch_writes, vec![1]);
        assert_eq!(f.variable_writes, vec![7]);
    }

    #[test]
    fn classifies_alias_vs_overwrite() {
        let src = r#"
            const _gainHp = Game_Battler.prototype.gainHp;
            Game_Battler.prototype.gainHp = function(v) { _gainHp.call(this, v); };
            Scene_Map.prototype.update = function() {};
        "#;
        let f = analyze_plugin(src);
        let gain = f
            .patches
            .iter()
            .find(|(m, _)| m == "Game_Battler.prototype.gainHp")
            .unwrap();
        assert!(!gain.1, "gainHp сохраняет alias → не перетирает");
        let upd = f
            .patches
            .iter()
            .find(|(m, _)| m == "Scene_Map.prototype.update")
            .unwrap();
        assert!(upd.1, "update без alias → перетирает");
    }

    #[test]
    fn extracts_register_command_with_const_prop() {
        let src = r#"
            const pn = "MyPlugin";
            PluginManager.registerCommand(pn, "doThing", args => {});
            PluginManager.registerCommand("MyPlugin", "other", () => {});
        "#;
        let f = analyze_plugin(src);
        assert!(f.registers_any);
        assert!(f.registry_complete);
        assert!(
            f.commands
                .contains(&("MyPlugin".to_string(), "doThing".to_string()))
        );
        assert!(
            f.commands
                .contains(&("MyPlugin".to_string(), "other".to_string()))
        );
    }

    #[test]
    fn marks_registry_incomplete_on_dynamic_registration() {
        let src = r#"
            PluginManager.registerCommand(someVar, computedName, fn);
        "#;
        let f = analyze_plugin(src);
        assert!(f.registers_any);
        assert!(!f.registry_complete);
        assert!(f.commands.is_empty());
    }

    #[test]
    fn script_binds_current_event_self_switch_write_and_read() {
        let w =
            analyze_script("$gameSelfSwitches.setValue([this._mapId, this._eventId, 'B'], true);");
        assert_eq!(w.self_switch_writes, vec!['B']);
        assert!(w.self_switch_reads.is_empty());

        let r =
            analyze_script("if ($gameSelfSwitches.value([this._mapId, this._eventId, 'A'])) {}");
        assert_eq!(r.self_switch_reads, vec!['A']);
        assert!(r.self_switch_writes.is_empty());
    }

    #[test]
    fn script_skips_foreign_or_computed_self_switch() {
        // Foreign event id (literal 9, not this._eventId) → not the current event → skip.
        let foreign = analyze_script("$gameSelfSwitches.setValue([this._mapId, 9, 'A'], true);");
        assert!(foreign.self_switch_writes.is_empty());
        // Computed key (an identifier) → skip.
        let computed = analyze_script("$gameSelfSwitches.setValue(key, true);");
        assert!(computed.self_switch_writes.is_empty());
    }

    #[test]
    fn script_extracts_global_switch_write() {
        let src = "$gameSwitches.setValue(12, false);";
        let f = analyze_script(src);
        assert_eq!(f.switch_writes, vec![12]);
    }

    #[test]
    fn extracts_literal_variable_reads() {
        let src = r#"
            const hp = $gameVariables.value(7) + $gameVariables.value(9);
            const s = "$gameVariables.value(98)"; // string -> skip
            $gameVariables.value(someVar); // computed -> skip
            $gameVariables.value(0); // 0 -> skip
        "#;
        let f = analyze_plugin(src);
        assert_eq!(f.variable_reads, vec![7, 9]);
        let s = analyze_script("if ($gameVariables.value(42) > 0) {}");
        assert_eq!(s.variable_reads, vec![42]);
    }

    #[test]
    fn extracts_reserve_common_event_literal() {
        let src = r#"
            $gameTemp.reserveCommonEvent(12);
            this._temp.reserveCommonEvent(0); // 0 -> skip
            $gameTemp.reserveCommonEvent(someVar); // computed -> skip
        "#;
        let f = analyze_plugin(src);
        assert_eq!(f.reserved_common_events, vec![12]);
        let s = analyze_script("$gameTemp.reserveCommonEvent(7);");
        assert_eq!(s.reserved_common_events, vec![7]);
    }

    #[test]
    fn reserve_method_definition_not_matched() {
        // A method definition (argument is an identifier) is not counted as a reservation.
        let src = "Game_Temp.prototype.reserveCommonEvent = function(commonEventId) {};";
        let f = analyze_plugin(src);
        assert!(f.reserved_common_events.is_empty());
    }

    #[test]
    fn tolerates_broken_js() {
        // Unclosed brackets / garbage — we don't panic, we return what we could.
        let src = "$gameSwitches.setValue(3, true); function( { [ unterminated";
        let f = analyze_plugin(src);
        assert!(f.switch_writes.contains(&3));
    }
}
