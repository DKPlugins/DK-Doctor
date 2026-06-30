//! Symbol table: switches and variables.
//!
//! Stores the read/write sites of each id plus the valid range bounds from
//! `System.switches`/`System.variables`. The "dead" and "uninitialized"
//! symbol rules are built on top of this.

use crate::ir::entity::EntityId;
use crate::ir::location::Location;
use rustc_hash::FxHashMap;

/// A symbol access site: location + the entity that produced it.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Site {
    /// Access location.
    pub location: Location,
    /// The entity within which the access occurred.
    pub entity: EntityId,
}

/// Information about a single symbol (switch or variable).
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SymbolInfo {
    /// Symbol id.
    pub id: u32,
    /// Name from `System.switches`/`variables` (an empty string is `Some("")`).
    pub name: Option<String>,
    /// Read sites.
    pub reads: Vec<Site>,
    /// Write sites.
    pub writes: Vec<Site>,
    /// Whether the symbol is declared by a plugin via an annotation
    /// (`@type switch`/`variable`, Tier A). Suppresses `uninitialized`.
    pub declared_by_plugin: bool,
    /// Whether the symbol is written by JS code of an enabled plugin with a
    /// literal id (`$gameSwitches.setValue(N, …)` / `$gameVariables.setValue(N, …)`,
    /// Tier B AST heuristic). Like `declared_by_plugin`, it means "the symbol is
    /// managed by a plugin at runtime": it suppresses `uninitialized` and lifts
    /// the `stuck-autorun` suspicion off pages enabled by this symbol.
    /// Intentionally **not** added as a write site (otherwise `dead-variables`
    /// would falsely conclude "written, not read").
    pub set_by_plugin: bool,
    /// Whether the variable is read by JS code of an enabled plugin or an event
    /// script with a literal id (`$gameVariables.value(N)`, Tier B AST heuristic).
    /// A variable written by a data command but consumed only from JS is **not**
    /// dead, so this flag suppresses the `dead-variables` finding. Intentionally
    /// **not** added as a read site (so it does not feed `uninitialized-symbols`).
    pub read_by_plugin: bool,
    /// Whether the switch is set to OFF anywhere (command 121 with the OFF value).
    /// If so, the gating condition of an Autorun/Parallel page may be cleared by
    /// another event, and `stuck-autorun` does NOT flag such a page (a fix for the
    /// flood on projects where global switches are toggled by events, e.g. F&H2).
    /// Switches only (variables have no unambiguous "turn off").
    pub ever_set_off: bool,
}

/// Project symbol table.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct SymbolTable {
    /// Switches by id.
    pub switches: FxHashMap<u32, SymbolInfo>,
    /// Variables by id.
    pub variables: FxHashMap<u32, SymbolInfo>,
    /// Maximum valid switch id (== `System.switches.len()-1`).
    pub max_switch_id: u32,
    /// Maximum valid variable id.
    pub max_variable_id: u32,
}

impl SymbolTable {
    /// Records the switch name, creating an entry if necessary.
    pub fn declare_switch(&mut self, id: u32, name: Option<String>) {
        let info = self.switches.entry(id).or_default();
        info.id = id;
        info.name = name;
    }

    /// Records the variable name, creating an entry if necessary.
    pub fn declare_variable(&mut self, id: u32, name: Option<String>) {
        let info = self.variables.entry(id).or_default();
        info.id = id;
        info.name = name;
    }

    /// Marks the switch as declared by a plugin (`@type switch`).
    ///
    /// Creates an entry if necessary. Removes the `uninitialized` finding —
    /// the plugin initializes the symbol at runtime.
    pub fn mark_switch_declared_by_plugin(&mut self, id: u32) {
        let info = self.switches.entry(id).or_default();
        info.id = id;
        info.declared_by_plugin = true;
    }

    /// Marks the variable as declared by a plugin (`@type variable`).
    pub fn mark_variable_declared_by_plugin(&mut self, id: u32) {
        let info = self.variables.entry(id).or_default();
        info.id = id;
        info.declared_by_plugin = true;
    }

    /// Marks the switch as written by plugin JS code (Tier B:
    /// `$gameSwitches.setValue(N, …)`). Creates an entry if necessary.
    pub fn mark_switch_set_by_plugin(&mut self, id: u32) {
        let info = self.switches.entry(id).or_default();
        info.id = id;
        info.set_by_plugin = true;
    }

    /// Marks the variable as written by plugin JS code (Tier B:
    /// `$gameVariables.setValue(N, …)`). Creates an entry if necessary.
    pub fn mark_variable_set_by_plugin(&mut self, id: u32) {
        let info = self.variables.entry(id).or_default();
        info.id = id;
        info.set_by_plugin = true;
    }

    /// Marks the variable as read by plugin/script JS code (Tier B:
    /// `$gameVariables.value(N)`). Creates an entry if necessary. Used to
    /// suppress `dead-variables` for variables consumed only from JS.
    pub fn mark_variable_read_by_plugin(&mut self, id: u32) {
        let info = self.variables.entry(id).or_default();
        info.id = id;
        info.read_by_plugin = true;
    }

    /// Marks the switch as being set to OFF anywhere (121 with the OFF value).
    /// Creates an entry if necessary.
    pub fn mark_switch_ever_set_off(&mut self, id: u32) {
        let info = self.switches.entry(id).or_default();
        info.id = id;
        info.ever_set_off = true;
    }

    /// Adds a switch read site.
    pub fn add_switch_read(&mut self, id: u32, site: Site) {
        let info = self.switches.entry(id).or_default();
        info.id = id;
        info.reads.push(site);
    }

    /// Adds a switch write site.
    pub fn add_switch_write(&mut self, id: u32, site: Site) {
        let info = self.switches.entry(id).or_default();
        info.id = id;
        info.writes.push(site);
    }

    /// Adds a variable read site.
    pub fn add_variable_read(&mut self, id: u32, site: Site) {
        let info = self.variables.entry(id).or_default();
        info.id = id;
        info.reads.push(site);
    }

    /// Adds a variable write site.
    pub fn add_variable_write(&mut self, id: u32, site: Site) {
        let info = self.variables.entry(id).or_default();
        info.id = id;
        info.writes.push(site);
    }
}
