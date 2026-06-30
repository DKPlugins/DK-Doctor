//! Plugin metadata — engine-agnostic structures populated by the adapter.
//!
//! The core does not know what an RPG Maker plugin is, but it stores universal
//! facts extracted by the adapter from annotations (Tier A): the registry of
//! registered commands and the load-order declarations
//! (`@base`/`@orderAfter`/`@orderBefore`). This data is read by rules registered
//! by the adapter (load-order, unknown plugin command), without bringing
//! RPG Maker semantics into the core.
//!
//! `@type switch`/`variable` declarations are NOT placed here: they are reflected
//! directly in [`crate::ir::SymbolTable`] via `declared_by_plugin`.

/// A command registered by a plugin (`@command` / `registerCommand`).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PluginCommand {
    /// Name of the plugin that declared the command (== `$plugins[].name`).
    pub plugin: String,
    /// Command name (== `registerCommand` key == on-disk 357 `[1]`).
    pub command: String,
}

/// A core method patched by a plugin (`X.prototype.m = …`, Tier B AST heuristic).
///
/// Engine-agnostic: the core sees only "plugin P assigns method M". If ≥2 enabled
/// plugins patch the same M and at least one overwrites (without a saved alias),
/// the `plugin-conflict` rule raises a finding.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MethodPatch {
    /// Full method chain, for example `Game_Battler.prototype.gainHp`.
    pub method: String,
    /// Plugin assigning the method (== `$plugins[].name`).
    pub plugin: String,
    /// `true` — overwrite (assigns the method without saving the original to an
    /// alias); `false` — alias-preserving patch (calls the previous
    /// implementation, cooperative).
    pub overwrites: bool,
}

/// A plugin command call from an event (command 356 MV / 357 MZ).
///
/// The adapter captures it while traversing command lists (before parsing the
/// registry), and the `unknown-plugin-command` rule checks it against
/// [`PluginMeta::commands`].
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PluginCommandCall {
    /// Plugin name (357 provides it structurally; `None` for 356 — MV raw string).
    pub plugin: Option<String>,
    /// Command name (357 `[1]`; for 356 — the first token of the raw string).
    pub command: String,
    /// `true` for structured 357 (exact matching), `false` for raw 356.
    pub structured: bool,
}

/// Load-order declarations of a single plugin from its header.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct PluginOrderDeps {
    /// Name of the plugin the dependencies belong to.
    pub plugin: String,
    /// `@base` — hard dependencies (must exist, be enabled, and load earlier).
    pub base: Vec<String>,
    /// `@orderAfter` — must load after the listed ones.
    pub order_after: Vec<String>,
    /// `@orderBefore` — must load before the listed ones.
    pub order_before: Vec<String>,
}

impl PluginOrderDeps {
    /// Whether there is at least one order declaration (otherwise the record is meaningless).
    pub fn is_empty(&self) -> bool {
        self.base.is_empty() && self.order_after.is_empty() && self.order_before.is_empty()
    }
}

/// Aggregate plugin metadata for the project (populated by the adapter, read by
/// adapter rules).
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct PluginMeta {
    /// Enabled plugins in load order (== order of `$plugins`, `status:true`).
    pub load_order: Vec<String>,
    /// Present but DISABLED plugins (`status:false`) — needed by the `missing-base`
    /// rule to distinguish "disabled" from "missing entirely".
    pub disabled: Vec<String>,
    /// Registry of registered commands (for checking 356/357). Populated from
    /// `@command` annotations (Tier A) and `PluginManager.registerCommand` (Tier B).
    pub commands: Vec<PluginCommand>,
    /// Plugins whose command registry is considered **complete**: their JS was
    /// parsed, all `registerCommand` calls had literal/constant arguments (or there
    /// are `@command` annotations), and ≥1 command was found. Only for these does
    /// `unknown-plugin-command` catch a command typo in an ENABLED plugin —
    /// otherwise (dynamic registration, untraced code) it must not flag.
    pub command_registry_known: Vec<String>,
    /// Load-order declarations per plugin.
    pub order_deps: Vec<PluginOrderDeps>,
    /// Core-method patches by plugins (Tier B) — input to the `plugin-conflict` rule.
    pub patches: Vec<MethodPatch>,
}

impl PluginMeta {
    /// Empty metadata (plugins were not parsed / are absent).
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether any plugins were parsed at all (there is at least one in load order).
    pub fn is_present(&self) -> bool {
        !self.load_order.is_empty()
    }
}
