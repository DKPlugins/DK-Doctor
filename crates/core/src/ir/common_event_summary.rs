//! Per-common-event behavioral summary — the interprocedural foundation.
//!
//! A [`CommonEventSummary`] captures, for a single common event, the facts other
//! rules need without re-walking its command list: which symbol classes it
//! touches, whether it transfers the player, whether it is opaque (contains a
//! script / plugin command), and which common events it calls. On top of the
//! per-event locals it carries two **transitive** verdicts computed over the
//! CommonEvent→CommonEvent call graph:
//!
//! - [`provides_exit`](CommonEventSummary::provides_exit) — the event (or anything
//!   it transitively calls) may change gating state, transfer, or do something
//!   opaque. `stuck-autorun` uses it to decide whether a `117` call on an Autorun
//!   page could hide an exit (if not, the call is a proven no-op and the page can
//!   still soft-lock).
//! - [`reachable`](CommonEventSummary::reachable) — the event can actually run:
//!   it is triggered (Autorun/Parallel), reserved, or reachable through the call
//!   graph from an entity that is not itself a common event (a map/troop event
//!   `117`, an effect-44 database reference, a plugin reference). `dead-common-event`
//!   uses it to flag mutually-calling clusters that no live caller ever reaches —
//!   a false-negative the direct-call check misses.
//!
//! Locals are derived from the IR edges; `opaque` is the one bit the adapter must
//! supply (it needs the numeric command codes, which live only in the adapter).

/// Behavioral summary of a single common event.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct CommonEventSummary {
    /// Common-event id.
    pub id: u32,
    /// Directly reads a switch.
    pub reads_switch: bool,
    /// Directly writes a switch.
    pub writes_switch: bool,
    /// Directly reads a variable.
    pub reads_variable: bool,
    /// Directly writes a variable.
    pub writes_variable: bool,
    /// Directly transfers the player (command 201).
    pub transfers: bool,
    /// Contains a script / plugin command (355/356/357) — its effect on game
    /// state is opaque to static analysis.
    pub opaque: bool,
    /// Common-event ids this event calls directly (command 117).
    pub calls: Vec<u32>,
    /// Transitive: this event, or any it (transitively) calls, may change gating
    /// state (write a switch/variable), transfer the player, or is opaque. When
    /// `false`, the event is proven to be a pure no-op with respect to page exits.
    pub provides_exit: bool,
    /// Transitive: the event can actually run (a triggered/reserved event, or one
    /// reached through the call graph from a non-common-event entity).
    pub reachable: bool,
}

impl CommonEventSummary {
    /// A fresh summary for `id` with the given `opaque` flag; all other facts default off.
    pub fn new(id: u32, opaque: bool) -> Self {
        Self {
            id,
            opaque,
            ..Default::default()
        }
    }

    /// The event's own (non-transitive) contribution to `provides_exit`: it writes
    /// a switch/variable, transfers, or is opaque.
    pub fn local_provides_exit(&self) -> bool {
        self.writes_switch || self.writes_variable || self.transfers || self.opaque
    }
}
