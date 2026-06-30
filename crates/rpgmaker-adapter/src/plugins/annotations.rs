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

use dk_doctor_core::ir::DbKind;

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
    /// Other types (`string`, `number`, struct<…>, …) — Tier A ignores them.
    Other,
}

/// Schema of a single `@param`.
#[derive(Clone, Debug)]
pub struct ParamSchema {
    /// Parameter name (the key in `plugins.js` `parameters`).
    pub name: String,
    /// Parameter type (`@type`).
    pub ty: ParamType,
    /// Whether it is an array (`@type X[]`).
    pub is_array: bool,
    /// `@dir` — directory for `@type file` (relative to the project root).
    pub dir: Option<String>,
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
}

impl ParamType {
    fn from_type_value(value: &str) -> (ParamType, bool) {
        let trimmed = value.trim();
        let (core, is_array) = match trimmed.strip_suffix("[]") {
            Some(core) => (core.trim(), true),
            None => (trimmed, false),
        };
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
        (ty, is_array)
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
                    cur_param = Some(ParamSchema {
                        name: rest.to_string(),
                        ty: ParamType::Other,
                        is_array: false,
                        dir: None,
                    });
                }
            }
            "@type" => {
                if let Some(p) = cur_param.as_mut() {
                    let (ty, is_array) = ParamType::from_type_value(rest);
                    p.ty = ty;
                    p.is_array = is_array;
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
