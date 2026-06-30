//! `CommonEvents.json` — array of `{id,name,trigger,switchId,list}`.

use crate::command::EventCommand;
use serde::Deserialize;

/// Common event.
#[derive(Clone, Debug, Deserialize)]
pub struct CommonEvent {
    /// Id (== index).
    #[serde(default)]
    pub id: u32,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// Trigger: 0 None, 1 Autorun, 2 Parallel.
    #[serde(default)]
    pub trigger: u32,
    /// Gate switch (READ when trigger!=0).
    #[serde(default, rename = "switchId")]
    pub switch_id: u32,
    /// Command list.
    #[serde(default)]
    pub list: Vec<EventCommand>,
}
