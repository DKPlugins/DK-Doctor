//! Table of self-switches — a separate namespace from global switches.
//!
//! A self-switch is addressed by the triple `(map_id, event_id, ch)` (`ch` ∈ A..D) and
//! lives only within its own event. Unlike global switches
//! from [`crate::ir::symbols::SymbolTable`], a self-switch is not declared in
//! `System.json`; its "existence" is determined by the presence of a write/read.

use crate::ir::symbols::Site;
use rustc_hash::FxHashMap;

/// Key of a self-switch: map, event, and channel (`'A'..'D'`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize)]
pub struct SelfSwitchKey {
    /// Id of the map the event belongs to.
    pub map_id: u32,
    /// Id of the event on the map.
    pub event_id: u32,
    /// Self-switch channel (`'A'`, `'B'`, `'C'`, `'D'`).
    pub ch: char,
}

impl SelfSwitchKey {
    /// Creates a self-switch key.
    pub fn new(map_id: u32, event_id: u32, ch: char) -> Self {
        Self {
            map_id,
            event_id,
            ch,
        }
    }
}

/// Details about a single self-switch: read and write sites.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SelfSwitchInfo {
    /// Read sites.
    pub reads: Vec<Site>,
    /// Write sites.
    pub writes: Vec<Site>,
}

/// Table of the project's self-switches — a separate namespace from global switches.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SelfSwitchTable {
    /// Self-switches keyed by `(map_id, event_id, ch)`.
    pub entries: FxHashMap<SelfSwitchKey, SelfSwitchInfo>,
}

impl SelfSwitchTable {
    /// Adds a self-switch read site.
    pub fn add_read(&mut self, key: SelfSwitchKey, site: Site) {
        self.entries.entry(key).or_default().reads.push(site);
    }

    /// Adds a self-switch write site.
    pub fn add_write(&mut self, key: SelfSwitchKey, site: Site) {
        self.entries.entry(key).or_default().writes.push(site);
    }
}
