//! Canonical event command envelope: `{code, indent, parameters}`.
//!
//! The command type is typed (`code`/`indent`), while the payload
//! (`parameters`) stays opaque (`Vec<Value>`) and is read positionally
//! via helpers — typing every command to its code in iter1 is overkill.

use serde::Deserialize;
use serde_json::Value;

/// A single command from an event list on disk.
#[derive(Clone, Debug, Deserialize)]
pub struct EventCommand {
    /// Numeric command code.
    #[serde(default)]
    pub code: u16,
    /// Indent level within the list.
    #[serde(default)]
    pub indent: i32,
    /// Opaque command parameters (read positionally).
    #[serde(default)]
    pub parameters: Vec<Value>,
}

impl EventCommand {
    /// Parameter `parameters[i]` as `u64`, if it is an integer.
    pub fn as_u64(&self, i: usize) -> Option<u64> {
        self.parameters.get(i).and_then(Value::as_u64)
    }

    /// Parameter `parameters[i]` as `i64`, if it is an integer (signed).
    /// Needed for the constant operands of 122/111, which can be negative.
    pub fn as_i64(&self, i: usize) -> Option<i64> {
        self.parameters.get(i).and_then(Value::as_i64)
    }

    /// Parameter `parameters[i]` as a string.
    pub fn as_str(&self, i: usize) -> Option<&str> {
        self.parameters.get(i).and_then(Value::as_str)
    }

    /// Parameter `parameters[i]` as a JSON object (e.g. an audio object).
    pub fn as_object(&self, i: usize) -> Option<&serde_json::Map<String, Value>> {
        self.parameters.get(i).and_then(Value::as_object)
    }

    /// The `name` field of the audio object `parameters[i]` (for commands 241/245/249/250).
    pub fn audio_name(&self, i: usize) -> Option<&str> {
        self.as_object(i)
            .and_then(|o| o.get("name"))
            .and_then(Value::as_str)
    }
}
