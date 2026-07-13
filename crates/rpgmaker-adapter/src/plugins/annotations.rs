//! Parser for the plugin annotation header block (`/*: … */`).
//!
//! Tier A, agnostic: we extract the standardized tags without knowing anything
//! about the specific plugin. Tolerant — missing tags are allowed, garbage does
//! not crash the parser. From each ENABLED plugin we extract a SCHEMA:
//!  - `@param <name>` + `@type` (switch/variable/file/`<db>` — state/common_event/
//!    actor/… + `[]`) + `@dir`;
//!  - `@command <name>` (MZ command registry);
//!  - `@base`/`@orderAfter`/`@orderBefore` (load-order dependencies).
//!
//! Parameter values come from `plugins.js` (here only the schema: what `@type`
//! a parameter has); their combination happens in [`super::collect`].
//!
//! When a parameter has **no** `@type` (the common case on MV, where the editor
//! never wrote one), [`infer_symbol_from_name`] recovers switch/variable/common-
//! event references from the parameter *name* suffix (`…Switch`/`…Variable`/
//! `…Common Event`). This is agnostic and suffix-only — see [`InferredKind`].

use dk_doctor_core::ir::DbKind;
use std::collections::HashMap;

/// Parameter type relevant to Tier A (we are indifferent to the other `@type`s).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamType {
    /// `@type switch` / `@type switch[]` — the value is switch id(s).
    Switch,
    /// `@type variable` / `@type variable[]` — the value is variable id(s).
    Variable,
    /// `@type file` / `@type file[]` — the value is asset path(s).
    File,
    /// `@type <db>` / `@type <db>[]` (`actor`/`item`/`state`/`common_event`/…) —
    /// the value is the DB record id of the corresponding kind. Standard MZ
    /// editor types, agnostic to the specific plugin.
    Db(DbKind),
    /// `@type struct<Name>` / `struct<Name>[]` — the value is a JSON-encoded
    /// object (or an array of them) whose fields follow the `/*~struct~Name:*/`
    /// schema. The struct's name is carried in [`ParamSchema::struct_name`].
    Struct,
    /// Other types (`string`, `number`, `note`, …) — Tier A ignores them.
    Other,
}

/// A switch/variable/common-event kind inferred from a parameter *name* alone.
///
/// Used only when a parameter carries **no explicit `@type`** (on MV `@type` is
/// absent, so the typed Tier A path is blind). Agnostic and suffix-only: the
/// value is then treated as a symbol/record id exactly as the corresponding
/// `@type` would treat it. Validated against the YEP/MV/MZ corpus (finding: 48
/// suffix params, 0 boolean-prefix collisions).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InferredKind {
    /// The value is switch id(s) (name ends with `…Switch`/`…Switches`).
    Switch,
    /// The value is variable id(s) (name ends with `…Variable`/`…Variables`).
    Variable,
    /// The value is common-event id(s) (name ends with `…Common Event`).
    CommonEvent,
}

/// The base words that name a symbol/record kind (used by both the id-strip guard
/// and the final suffix match, so `"Common Event ID"` strips like `"Switch ID"`).
fn ends_with_symbol_word(base: &str) -> bool {
    base.ends_with("switch")
        || base.ends_with("switches")
        || base.ends_with("variable")
        || base.ends_with("variables")
        || base.ends_with("common event")
        || base.ends_with("common events")
        || base.ends_with("commonevent")
        || base.ends_with("commonevents")
}

/// Strips a decorative trailing `id`/`ids` token so the base word ends the string
/// (`"Dawn Switch ID"` → `"dawn switch"`, `"switchId"` → `"switch"`,
/// `"Common Event ID"` → `"common event"`). The strip is accepted only when it
/// exposes a symbol word — otherwise ordinary words like `"grid"` must not be
/// mutilated.
fn strip_id_suffix(s: &str) -> &str {
    for tail in [" ids", " id", "ids", "id"] {
        if let Some(base) = s.strip_suffix(tail) {
            let base = base.trim_end();
            if ends_with_symbol_word(base) {
                return base;
            }
        }
    }
    s
}

/// Infers a switch/variable/common-event kind from an **untyped** parameter name
/// by its trailing token. Returns `None` when no suffix matches.
///
/// Whole-token by construction: `str::ends_with` already rejects `"Switcher"`
/// (ends with `switcher`, not `switch`) and `"SwitchActorText"` (the token is a
/// prefix, not the tail), the two false-positive traps seen in the corpus.
/// Common-event is checked first — its suffixes are disjoint from the others.
///
/// Retained for tests and as a documented building block, but **no longer used to
/// suppress findings**: the parameter name and value both come from the analyzed
/// project, so trusting a name suffix to mark an id plugin-managed let a hostile
/// project hide `uninitialized-symbols`/`stuck-autorun`/`dead-common-event`
/// findings. Only an explicit `@type switch`/`variable`/`common_event` suppresses.
#[allow(dead_code)]
pub fn infer_symbol_from_name(name: &str) -> Option<InferredKind> {
    let lower = name.trim().to_ascii_lowercase();
    let base = strip_id_suffix(&lower);
    let ends_any = |suffixes: &[&str]| suffixes.iter().any(|s| base.ends_with(s));
    if ends_any(&[
        "common event",
        "common events",
        "commonevent",
        "commonevents",
    ]) {
        Some(InferredKind::CommonEvent)
    } else if ends_any(&["switch", "switches"]) {
        Some(InferredKind::Switch)
    } else if ends_any(&["variable", "variables"]) {
        Some(InferredKind::Variable)
    } else {
        None
    }
}

/// Schema of a single `@param`.
#[derive(Clone, Debug)]
pub struct ParamSchema {
    /// Parameter name (the key in `plugins.js` `parameters`).
    pub name: String,
    /// Parameter type (`@type`).
    pub ty: ParamType,
    /// Struct name for `@type struct<Name>` (else `None`) — the key into
    /// [`PluginAnnotations::structs`].
    pub struct_name: Option<String>,
    /// Whether an explicit `@type` tag was present (even an unrecognized one like
    /// `boolean`/`string`/`struct`). Guards name-alias inference: an explicit type
    /// always wins over the name, so `@type boolean` on a `…Switch` param is never
    /// re-interpreted as a switch id.
    pub has_explicit_type: bool,
    /// Whether it is an array (`@type X[]`).
    pub is_array: bool,
    /// `@dir` — directory for `@type file` (relative to the project root).
    pub dir: Option<String>,
}

impl ParamSchema {
    /// A fresh, untyped `@param <name>` schema (before its `@type`/`@dir` lines).
    fn new(name: String) -> Self {
        Self {
            name,
            ty: ParamType::Other,
            struct_name: None,
            has_explicit_type: false,
            is_array: false,
            dir: None,
        }
    }
}

/// Parsed plugin header schema.
#[derive(Clone, Debug, Default)]
pub struct PluginAnnotations {
    /// Parameters with their types.
    pub params: Vec<ParamSchema>,
    /// Names of registered commands (`@command`).
    pub commands: Vec<String>,
    /// `@base` — hard dependencies.
    pub base: Vec<String>,
    /// `@orderAfter`.
    pub order_after: Vec<String>,
    /// `@orderBefore`.
    pub order_before: Vec<String>,
    /// Struct schemas keyed by name (`/*~struct~Name:*/` blocks) — the field
    /// layout referenced by `@type struct<Name>` parameters.
    pub structs: HashMap<String, Vec<ParamSchema>>,
}

impl ParamType {
    /// Parses a `@type` value into `(type, is_array, struct_name)`.
    fn from_type_value(value: &str) -> (ParamType, bool, Option<String>) {
        let trimmed = value.trim();
        let (core, is_array) = match trimmed.strip_suffix("[]") {
            Some(core) => (core.trim(), true),
            None => (trimmed, false),
        };
        // struct<Name> — the value is a JSON object following the named schema.
        if let Some(rest) = core.strip_prefix("struct<")
            && let Some(name) = rest.strip_suffix('>')
        {
            return (ParamType::Struct, is_array, Some(name.trim().to_string()));
        }
        let ty = match core {
            "switch" => ParamType::Switch,
            "variable" => ParamType::Variable,
            "file" => ParamType::File,
            "actor" => ParamType::Db(DbKind::Actor),
            "class" => ParamType::Db(DbKind::Class),
            "skill" => ParamType::Db(DbKind::Skill),
            "item" => ParamType::Db(DbKind::Item),
            "weapon" => ParamType::Db(DbKind::Weapon),
            "armor" => ParamType::Db(DbKind::Armor),
            "enemy" => ParamType::Db(DbKind::Enemy),
            "troop" => ParamType::Db(DbKind::Troop),
            "state" => ParamType::Db(DbKind::State),
            "animation" => ParamType::Db(DbKind::Animation),
            "tileset" => ParamType::Db(DbKind::Tileset),
            "common_event" => ParamType::Db(DbKind::CommonEvent),
            _ => ParamType::Other,
        };
        (ty, is_array, None)
    }
}

/// Extracts the first `/*: … */` annotation header block from the source.
///
/// Prefers the default `/*:` (without a language suffix like `/*:ja`), but when
/// it is absent takes the first localized one — the tags are structurally identical.
fn extract_header_block(src: &str) -> Option<&str> {
    let mut best: Option<&str> = None;
    let bytes = src.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' && bytes[i + 2] == b':' {
            // End of the block.
            if let Some(rel_end) = src[i..].find("*/") {
                let block = &src[i + 3..i + rel_end];
                // Default `/*:` — right after the colon comes a newline or a space
                // (not a language-tag letter like `ja`/`ru`).
                let is_default = block
                    .chars()
                    .next()
                    .is_none_or(|c| c == '\n' || c == '\r' || c == ' ' || c == '\t');
                if is_default {
                    return Some(block);
                }
                best.get_or_insert(block);
                i += rel_end + 2;
                continue;
            }
        }
        i += 1;
    }
    best
}

/// Strips leading `*` and whitespace from a comment line (`* @param x` → `@param x`).
fn clean_line(line: &str) -> &str {
    let t = line.trim_start();
    let t = t.strip_prefix('*').unwrap_or(t);
    t.trim()
}

/// Splits a tag line into `(@tag, rest)`.
fn split_tag(line: &str) -> Option<(&str, &str)> {
    if !line.starts_with('@') {
        return None;
    }
    match line.split_once(char::is_whitespace) {
        Some((tag, rest)) => Some((tag, rest.trim())),
        None => Some((line, "")),
    }
}

/// Parses the plugin header into [`PluginAnnotations`] (tolerantly).
pub fn parse(src: &str) -> PluginAnnotations {
    let mut out = PluginAnnotations::default();
    let Some(block) = extract_header_block(src) else {
        return out;
    };

    // The currently open `@param`/`@command` context: its `@type`/`@dir`/… come
    // on the following lines until a new `@param`/`@command`/`@arg`.
    let mut cur_param: Option<ParamSchema> = None;

    let flush_param = |cur: &mut Option<ParamSchema>, out: &mut PluginAnnotations| {
        if let Some(p) = cur.take() {
            out.params.push(p);
        }
    };

    for raw in block.lines() {
        let line = clean_line(raw);
        let Some((tag, rest)) = split_tag(line) else {
            continue;
        };
        match tag {
            "@param" => {
                flush_param(&mut cur_param, &mut out);
                if !rest.is_empty() {
                    cur_param = Some(ParamSchema::new(rest.to_string()));
                }
            }
            "@type" => {
                if let Some(p) = cur_param.as_mut() {
                    let (ty, is_array, struct_name) = ParamType::from_type_value(rest);
                    p.ty = ty;
                    p.is_array = is_array;
                    p.struct_name = struct_name;
                    p.has_explicit_type = true;
                }
            }
            "@dir" => {
                if let Some(p) = cur_param.as_mut()
                    && !rest.is_empty()
                {
                    p.dir = Some(rest.trim_matches('/').to_string());
                }
            }
            "@command" => {
                // A command closes the current param context.
                flush_param(&mut cur_param, &mut out);
                if !rest.is_empty() {
                    out.commands.push(rest.to_string());
                }
            }
            "@arg" => {
                // A command argument — closes the param context, but in Tier A we
                // do not need the argument itself (only the command registry).
                flush_param(&mut cur_param, &mut out);
            }
            "@base" => {
                flush_param(&mut cur_param, &mut out);
                if !rest.is_empty() {
                    out.base.push(rest.to_string());
                }
            }
            "@orderAfter" => {
                flush_param(&mut cur_param, &mut out);
                if !rest.is_empty() {
                    out.order_after.push(rest.to_string());
                }
            }
            "@orderBefore" => {
                flush_param(&mut cur_param, &mut out);
                if !rest.is_empty() {
                    out.order_before.push(rest.to_string());
                }
            }
            _ => {}
        }
    }
    flush_param(&mut cur_param, &mut out);

    // Struct schemas: separate `/*~struct~Name:*/` blocks referenced by
    // `@type struct<Name>`. Default (unsuffixed) blocks win over localized ones.
    for (name, is_default, block) in extract_struct_blocks(src) {
        if is_default || !out.structs.contains_key(&name) {
            out.structs.insert(name, parse_struct_field_params(block));
        }
    }
    out
}

/// Parses only the `@param`/`@type`/`@dir` lines of a `/*~struct~Name:*/` block
/// into field schemas (structs carry no `@command`/`@base`).
fn parse_struct_field_params(block: &str) -> Vec<ParamSchema> {
    let mut fields = Vec::new();
    let mut cur: Option<ParamSchema> = None;
    for raw in block.lines() {
        let line = clean_line(raw);
        let Some((tag, rest)) = split_tag(line) else {
            continue;
        };
        match tag {
            "@param" => {
                if let Some(p) = cur.take() {
                    fields.push(p);
                }
                if !rest.is_empty() {
                    cur = Some(ParamSchema::new(rest.to_string()));
                }
            }
            "@type" => {
                if let Some(p) = cur.as_mut() {
                    let (ty, is_array, struct_name) = ParamType::from_type_value(rest);
                    p.ty = ty;
                    p.is_array = is_array;
                    p.struct_name = struct_name;
                    p.has_explicit_type = true;
                }
            }
            "@dir" => {
                if let Some(p) = cur.as_mut()
                    && !rest.is_empty()
                {
                    p.dir = Some(rest.trim_matches('/').to_string());
                }
            }
            _ => {}
        }
    }
    if let Some(p) = cur {
        fields.push(p);
    }
    fields
}

/// Extracts every `/*~struct~Name:*/` block as `(name, is_default, body)`.
///
/// `is_default` is `false` for a localized block whose name is followed by a
/// language tag (`/*~struct~Name:ja`) — the default (unsuffixed) block is
/// preferred, mirroring [`extract_header_block`].
fn extract_struct_blocks(src: &str) -> Vec<(String, bool, &str)> {
    const NEEDLE: &str = "/*~struct~";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = src[from..].find(NEEDLE) {
        let after = from + rel + NEEDLE.len();
        let Some(colon) = src[after..].find(':') else {
            break;
        };
        let name = src[after..after + colon].trim().to_string();
        let body_start = after + colon + 1;
        let Some(end_rel) = src[body_start..].find("*/") else {
            break;
        };
        let body = &src[body_start..body_start + end_rel];
        // A default block has a newline/whitespace right after the colon; a
        // localized one (`:ja`) starts with a language-tag letter.
        let is_default = body
            .chars()
            .next()
            .is_none_or(|c| c == '\n' || c == '\r' || c == ' ' || c == '\t');
        out.push((name, is_default, body));
        from = body_start + end_rel + 2;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
//=============================================================================
// MyPlugin.js
//=============================================================================
/*:
 * @target MZ
 * @plugindesc A sample plugin.
 * @author Someone
 * @base CoreEngine
 * @orderAfter OtherPlugin
 * @orderBefore LatePlugin
 *
 * @param EnableSwitch
 * @text Enable Switch
 * @type switch
 * @default 5
 *
 * @param ManagedVars
 * @type variable[]
 * @default ["10","11"]
 *
 * @param Portrait
 * @type file
 * @dir img/pictures
 * @default hero_a
 *
 * @param Title
 * @type string
 * @default Hello
 *
 * @command openShop
 * @text Open Shop
 * @arg goodsActor
 * @type actor
 *
 * @help Lots of help text here.
 */

(function() {})();
"#;

    #[test]
    fn extracts_param_types_command_and_order_deps() {
        let a = parse(SAMPLE);

        // @param with types.
        let by_name = |n: &str| a.params.iter().find(|p| p.name == n).unwrap();
        let sw = by_name("EnableSwitch");
        assert_eq!(sw.ty, ParamType::Switch);
        assert!(!sw.is_array);

        let vars = by_name("ManagedVars");
        assert_eq!(vars.ty, ParamType::Variable);
        assert!(vars.is_array);

        let portrait = by_name("Portrait");
        assert_eq!(portrait.ty, ParamType::File);
        assert_eq!(portrait.dir.as_deref(), Some("img/pictures"));

        let title = by_name("Title");
        assert_eq!(title.ty, ParamType::Other);

        // @command.
        assert_eq!(a.commands, vec!["openShop".to_string()]);

        // Load-order dependencies.
        assert_eq!(a.base, vec!["CoreEngine".to_string()]);
        assert_eq!(a.order_after, vec!["OtherPlugin".to_string()]);
        assert_eq!(a.order_before, vec!["LatePlugin".to_string()]);
    }

    #[test]
    fn maps_db_typed_params() {
        let src = r#"/*:
 * @param OnLangChange
 * @type common_event
 * @default 0
 *
 * @param StartingStates
 * @type state[]
 * @default []
 *
 * @param Boss
 * @type enemy
 */"#;
        let a = parse(src);
        let by_name = |n: &str| a.params.iter().find(|p| p.name == n).unwrap();
        assert_eq!(
            by_name("OnLangChange").ty,
            ParamType::Db(DbKind::CommonEvent)
        );
        let states = by_name("StartingStates");
        assert_eq!(states.ty, ParamType::Db(DbKind::State));
        assert!(states.is_array);
        assert_eq!(by_name("Boss").ty, ParamType::Db(DbKind::Enemy));
    }

    #[test]
    fn parses_struct_type_and_its_field_schema() {
        let src = r#"/*:
 * @param Templates
 * @type struct<Template>[]
 * @default []
 *
 * @param Single
 * @type struct<Template>
 */
/*~struct~Template:
 * @param Trigger Switch
 * @type switch
 * @param Reward Common Event
 * @type common_event
 * @param Label
 * @type string
 */"#;
        let a = parse(src);
        let by_name = |n: &str| a.params.iter().find(|p| p.name == n).unwrap();
        let templates = by_name("Templates");
        assert_eq!(templates.ty, ParamType::Struct);
        assert!(templates.is_array);
        assert_eq!(templates.struct_name.as_deref(), Some("Template"));
        assert!(templates.has_explicit_type);
        assert_eq!(by_name("Single").struct_name.as_deref(), Some("Template"));
        // The referenced struct's field schema is captured.
        let fields = a.structs.get("Template").expect("struct schema present");
        let field = |n: &str| fields.iter().find(|p| p.name == n).unwrap();
        assert_eq!(field("Trigger Switch").ty, ParamType::Switch);
        assert_eq!(
            field("Reward Common Event").ty,
            ParamType::Db(DbKind::CommonEvent)
        );
        assert_eq!(field("Label").ty, ParamType::Other);
    }

    #[test]
    fn prefers_default_struct_block_over_localized() {
        let src = r#"/*:
 * @param P
 * @type struct<S>
 */
/*~struct~S:ja
 * @param Loc
 * @type variable
 */
/*~struct~S:
 * @param Def
 * @type switch
 */"#;
        let a = parse(src);
        let fields = a.structs.get("S").expect("struct S present");
        // The default block wins: it has Def(switch), not Loc(variable).
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "Def");
        assert_eq!(fields[0].ty, ParamType::Switch);
    }

    #[test]
    fn tolerates_no_header_block() {
        let a = parse("(function(){ /* not an annotation */ })();");
        assert!(a.params.is_empty());
        assert!(a.commands.is_empty());
    }

    #[test]
    fn falls_back_to_localized_block() {
        let src = r#"/*:ja
 * @param Sw
 * @type switch
 */"#;
        let a = parse(src);
        assert_eq!(a.params.len(), 1);
        assert_eq!(a.params[0].ty, ParamType::Switch);
    }

    #[test]
    fn records_explicit_type_flag() {
        // A param with @type (even an unrecognized one) is flagged; a bare @param is not.
        let src = r#"/*:
 * @param BattleSwitch
 * @type boolean
 * @param CarriedVariable
 */"#;
        let a = parse(src);
        let by_name = |n: &str| a.params.iter().find(|p| p.name == n).unwrap();
        assert!(by_name("BattleSwitch").has_explicit_type);
        assert_eq!(by_name("BattleSwitch").ty, ParamType::Other); // boolean → Other
        assert!(!by_name("CarriedVariable").has_explicit_type);
    }

    #[test]
    fn infers_switch_variable_common_event_by_suffix() {
        use InferredKind::*;
        // Positive: whole-token suffixes across spacing/camelCase/plural/ID forms.
        for (name, want) in [
            ("Battle Switch", Switch),
            ("Non-Local Switch", Switch),
            ("cheaterSwitch", Switch),
            ("OnSwitches", Switch),
            ("Dawn Switch ID", Switch),
            ("Day Week Switches IDs", Switch),
            ("mainSwitchId", Switch),
            ("Carried Variable", Variable),
            ("Latest Button Variable", Variable),
            ("variableId", Variable),
            ("Hour Variable ID", Variable),
            ("Alert Common Event", CommonEvent),
            ("CommonEvent", CommonEvent),
            ("On Language Change Common Event", CommonEvent),
            ("Alert Common Event ID", CommonEvent),
            ("CommonEventID", CommonEvent),
            ("Start Common Events IDs", CommonEvent),
            ("commonEventId", CommonEvent), // Galv (untyped, real CE id)
        ] {
            assert_eq!(
                infer_symbol_from_name(name),
                Some(want),
                "{name} should infer {want:?}"
            );
        }
    }

    #[test]
    fn does_not_infer_from_non_suffix_names() {
        // "Switcher"/"SwitchActorText" traps, a real word ending in "id",
        // and plain unrelated names must NOT match.
        for name in [
            "Message Switcher",
            "SwitchActorText",
            "Grid",
            "Show Percents",
            "Window Width",
            "commonEventTime", // Galv: a duration, "…Time" is not a suffix match
            "Enable",          // no suffix at all
            "",
        ] {
            assert_eq!(infer_symbol_from_name(name), None, "{name} must not infer");
        }
    }

    #[test]
    fn prefers_default_over_localized_block() {
        let src = r#"/*:ja
 * @param Loc
 * @type variable
 */
/*:
 * @param Def
 * @type switch
 */"#;
        let a = parse(src);
        // The default block is selected — it contains only Def(switch).
        assert_eq!(a.params.len(), 1);
        assert_eq!(a.params[0].name, "Def");
        assert_eq!(a.params[0].ty, ParamType::Switch);
    }
}
