//! `js/plugins.js` — NOT JSON: `var $plugins = [ … ];` with a header.
//!
//! The parser is robust to brackets/escapes inside strings: we find `var $plugins =`,
//! then scan characters, tracking string context and escaping,
//! until the outer `[ … ]` closes. The slice found is a valid JSON array,
//! which we feed to serde_json. Array order = load order.
//!
//! Real case: Fear & Hunger 2 Termina — 118 plugins, parameters contain
//! `]` inside strings, which broke the naive `find('[')..rfind(']')`.

use serde::Deserialize;
use std::collections::BTreeMap;

/// A plugin entry in `$plugins`.
#[derive(Clone, Debug, Deserialize)]
pub struct PluginEntry {
    /// Plugin file name without `.js`.
    #[serde(default)]
    pub name: String,
    /// Whether the plugin is enabled (only enabled ones register commands).
    #[serde(default)]
    pub status: bool,
    /// `@plugindesc` from the header (as serialized by the editor).
    #[serde(default)]
    pub description: String,
    /// Plugin parameters: name → string value (everything is a string;
    /// struct/array parameters are JSON strings inside strings).
    ///
    /// Tolerant to non-string values: the MZ editor puts service
    /// UI-state keys (e.g. `__collapsed` — an array of names of collapsed groups),
    /// whose values are NOT strings. A naive `BTreeMap<String, String>` would break parsing
    /// of the WHOLE `$plugins` on a single such entry (real case: an authored MZ project
    /// with `DKTools_Localization`/`DK_Quest_System`). Here non-string values
    /// are simply discarded (see [`deserialize_string_params`]).
    #[serde(default, deserialize_with = "deserialize_string_params")]
    pub parameters: BTreeMap<String, String>,
}

/// Deserializes `parameters`, keeping only string values and discarding
/// non-string ones (arrays/objects/numbers/booleans) — editor service keys
/// (`__collapsed` etc.) must not break parsing of the entire plugins array.
fn deserialize_string_params<'de, D>(de: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: BTreeMap<String, serde_json::Value> = BTreeMap::deserialize(de)?;
    Ok(raw
        .into_iter()
        .filter_map(|(k, v)| match v {
            serde_json::Value::String(s) => Some((k, s)),
            _ => None,
        })
        .collect())
}

/// Extracts the plugins array from the text of `plugins.js`.
///
/// Returns plugins in load order; separator entries
/// (`name` empty/made of dashes) are filtered out. Tolerant to garbage: on a parsing
/// failure it returns an empty vector rather than an error.
pub fn parse(text: &str) -> Vec<PluginEntry> {
    let Some(array) = extract_array(text) else {
        return Vec::new();
    };
    let parsed: Vec<PluginEntry> = serde_json::from_str(array).unwrap_or_default();
    parsed.into_iter().filter(|p| !is_separator(p)).collect()
}

/// A separator entry (`name` empty or made of dashes/spaces).
fn is_separator(p: &PluginEntry) -> bool {
    let trimmed = p.name.trim_matches('-').trim();
    trimmed.is_empty()
}

/// Finds the `[ … ]` slice of the `$plugins` array, correctly skipping brackets and
/// `]` characters inside JSON strings (accounting for escaping `\"`, `\\`).
///
/// Prefers the array after `var $plugins =`; if the marker is not found, it takes
/// the first `[` in the text (tolerance to non-standard headers).
fn extract_array(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let search_from = text.find("$plugins").unwrap_or(0);
    // The first '[' starting from the marker (or from the beginning).
    let start = text[search_from..].find('[').map(|i| search_from + i)?;

    let mut depth: i32 = 0;
    let mut in_str = false;
    let mut escaped = false;
    let mut i = start;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&text[start..=i]);
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_and_filters_separators() {
        let text = r#"// header
var $plugins =
[
{"name":"PluginA","status":true,"description":"","parameters":{}},
{"name":"--------","status":false,"description":"","parameters":{}},
{"name":"PluginB","status":false,"description":"","parameters":{}}
];"#;
        let plugins = parse(text);
        assert_eq!(plugins.len(), 2);
        assert_eq!(plugins[0].name, "PluginA");
        assert!(plugins[0].status);
        assert_eq!(plugins[1].name, "PluginB");
    }

    #[test]
    fn tolerates_brackets_inside_param_strings() {
        // The parameter contains ']' and '[' inside a string value — a naive
        // rfind(']') would truncate the array prematurely. Plus an escaped
        // quote inside a string.
        let text = r#"var $plugins =
[
{"name":"Owns","status":true,"description":"desc with ] bracket","parameters":{"List":"[\"1\",\"2\"]","Quote":"a\"b]"}},
{"name":"Last","status":true,"description":"","parameters":{}}
];
"#;
        let plugins = parse(text);
        assert_eq!(plugins.len(), 2);
        assert_eq!(plugins[0].name, "Owns");
        assert_eq!(plugins[0].parameters.get("List").unwrap(), r#"["1","2"]"#);
        assert_eq!(plugins[0].parameters.get("Quote").unwrap(), "a\"b]");
        assert_eq!(plugins[1].name, "Last");
    }

    #[test]
    fn empty_on_garbage() {
        assert!(parse("not javascript at all").is_empty());
        assert!(parse("var $plugins = ").is_empty());
    }

    #[test]
    fn tolerates_non_string_param_values() {
        // The MZ editor puts `__collapsed` as an array — it must not break parsing
        // of the whole array; string parameters are kept, `__collapsed` is discarded.
        let text = r#"var $plugins =
[
{"name":"DKTools_Localization","status":true,"description":"","parameters":{"__collapsed":["A","B"],"Cache":"true"}},
{"name":"DK_Message_Busts","status":true,"description":"","parameters":{"bustsFolder":"img/pictures/busts/"}}
];
"#;
        let plugins = parse(text);
        assert_eq!(
            plugins.len(),
            2,
            "обе записи разобраны несмотря на __collapsed"
        );
        let loc = &plugins[0];
        assert_eq!(loc.name, "DKTools_Localization");
        assert_eq!(
            loc.parameters.get("Cache").map(String::as_str),
            Some("true")
        );
        assert!(
            !loc.parameters.contains_key("__collapsed"),
            "не-строковое значение отброшено"
        );
        assert_eq!(
            plugins[1].parameters.get("bustsFolder").map(String::as_str),
            Some("img/pictures/busts/")
        );
    }

    #[test]
    fn handles_missing_marker_falls_back_to_first_array() {
        let text = r#"[{"name":"X","status":true,"description":"","parameters":{}}]"#;
        let plugins = parse(text);
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "X");
    }
}
