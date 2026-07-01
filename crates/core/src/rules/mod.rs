//! Rule engine: the [`Rule`] trait, the [`RuleCtx`] context and the [`Registry`].
//!
//! Rules take `&Ir`, mutate nothing and return `Vec<Finding>`.
//! Adding a rule is one file + one `Box::new(...)` line in
//! [`Registry::with_builtin`]. In iter1 the registry is empty (rules are added later).

use crate::finding::{Category, Finding};
use crate::ir::Ir;

pub mod blocked_tile;
pub mod broken_assets;
pub mod broken_transfer;
pub mod circular_gate;
pub mod cyclic_common_events;
pub mod db_reachability;
pub mod dead_code_after_exit;
pub mod dead_common_event;
pub mod dead_self_switch;
pub mod dead_variables;
pub mod empty_event_page;
pub mod impossible_condition;
pub mod missing_base;
pub mod orphan_assets;
pub mod page_index;
pub mod picture_lifecycle;
pub mod plugin_conflict;
pub mod plugin_load_order;
pub mod referential_integrity;
pub mod shadowed_page;
pub mod stuck_autorun;
pub mod uninit_symbols;
pub mod unknown_plugin_command;
pub mod unreachable_maps;
pub mod unreachable_self_switch;
pub mod vehicle_start_map;

/// Context in which a rule runs.
pub struct RuleCtx<'a> {
    /// The IR being analyzed.
    pub ir: &'a Ir,
    /// Whether symbol declarations from plugins are available (always `false` in iter1).
    pub plugin_decls_available: bool,
    /// Numeric codes of "event exit" commands that the `dead-code-after-exit`
    /// rule treats as terminating the flow (for RPG Maker — `115`).
    ///
    /// The core does not know the semantics of the codes: the adapter passes the
    /// concrete numbers here, keeping the core engine-independent. An empty slice
    /// disables the rule.
    pub exit_command_codes: &'a [u16],
    /// Codes of commands whose presence on a page means "the page may change
    /// state / exit through a mechanism that static analysis cannot trace" — for
    /// RPG Maker these are a common event call (`117`), plugin commands (`356`/`357`) and
    /// an arbitrary script (`355`). The `stuck-autorun` rule does NOT flag such
    /// pages (the exit may be hidden inside the common event / plugin / computed
    /// script → confidence is too low). An empty slice disables the filter.
    pub opaque_exit_codes: &'a [u16],
    /// Numeric codes of label commands (a potential "jump" target). For RPG Maker
    /// this is `118` (Label) — the target of the `119` (Jump to Label) command. The
    /// `dead-code-after-exit` rule stops marking the dead tail at such a
    /// command: code following the label may be reachable via a jump that bypasses the exit.
    /// An empty slice disables label tracking.
    pub label_command_codes: &'a [u16],
    /// Codes of "no-op" commands that carry no behavior and only mark structure —
    /// for RPG Maker this is `0`, the empty command the editor appends at the end of
    /// every command list and every indent block (a block/list terminator). The
    /// `dead-code-after-exit` rule skips them: such a command after an exit is not
    /// real dead code, just the block terminator. An empty slice disables the filter.
    pub noop_command_codes: &'a [u16],
}

impl<'a> RuleCtx<'a> {
    /// Creates a context (without exit codes).
    ///
    /// `plugin_decls_available` is derived from the IR: true if the adapter parsed
    /// at least one plugin (`ir.plugin_meta` is non-empty). This way rules know that
    /// the `declared_by_plugin` signal is relevant.
    pub fn new(ir: &'a Ir) -> Self {
        Self {
            ir,
            plugin_decls_available: ir.plugin_meta.is_present(),
            exit_command_codes: &[],
            opaque_exit_codes: &[],
            label_command_codes: &[],
            noop_command_codes: &[],
        }
    }

    /// Context with the given exit command codes (for the adapter/CLI and the
    /// `dead-code-after-exit` rule tests). The "untraceable exit" codes are empty.
    pub fn with_exit_codes(ir: &'a Ir, exit_command_codes: &'a [u16]) -> Self {
        Self {
            ir,
            plugin_decls_available: ir.plugin_meta.is_present(),
            exit_command_codes,
            opaque_exit_codes: &[],
            label_command_codes: &[],
            noop_command_codes: &[],
        }
    }

    /// Full context: exit codes (`dead-code-after-exit`), untraceable-exit codes
    /// (`stuck-autorun`) and label codes (jump targets).
    /// Used by the CLI/adapter.
    pub fn with_codes(
        ir: &'a Ir,
        exit_command_codes: &'a [u16],
        opaque_exit_codes: &'a [u16],
        label_command_codes: &'a [u16],
    ) -> Self {
        Self {
            ir,
            plugin_decls_available: ir.plugin_meta.is_present(),
            exit_command_codes,
            opaque_exit_codes,
            label_command_codes,
            noop_command_codes: &[],
        }
    }

    /// Sets the no-op/structural command codes (RPG Maker `0` — the block/list
    /// terminator the editor appends). `dead-code-after-exit` skips them so a
    /// trailing empty command after an exit is not reported as dead code. Returns
    /// `self` for chaining onto [`Self::with_codes`].
    pub fn with_noop_codes(mut self, noop_command_codes: &'a [u16]) -> Self {
        self.noop_command_codes = noop_command_codes;
        self
    }
}

/// A diagnostic rule: a pure function over the IR.
pub trait Rule: Send + Sync {
    /// Stable rule id (== the `Finding::rule` field).
    fn id(&self) -> &'static str;
    /// Category of this rule's findings.
    fn category(&self) -> Category;
    /// Runs the rule and returns the findings.
    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding>;
}

/// Rule registry.
pub struct Registry {
    rules: Vec<Box<dyn Rule>>,
}

impl Registry {
    /// Registry with all built-in rules of iteration 1.
    pub fn with_builtin() -> Self {
        Self {
            rules: vec![
                Box::new(dead_variables::DeadVariables),
                Box::new(uninit_symbols::UninitSymbols),
                Box::new(broken_transfer::BrokenTransfer),
                Box::new(impossible_condition::ImpossibleCondition),
                Box::new(unreachable_maps::UnreachableMaps),
                Box::new(referential_integrity::ReferentialIntegrity),
                Box::new(broken_assets::BrokenAssets),
                Box::new(orphan_assets::OrphanAssets),
                Box::new(dead_code_after_exit::DeadCodeAfterExit),
                Box::new(dead_self_switch::DeadSelfSwitch),
                Box::new(unreachable_self_switch::UnreachableSelfSwitch),
                Box::new(dead_common_event::DeadCommonEvent),
                Box::new(cyclic_common_events::CyclicCommonEvents),
                Box::new(shadowed_page::ShadowedPage),
                Box::new(stuck_autorun::StuckAutorun),
                Box::new(plugin_load_order::PluginLoadOrder),
                Box::new(missing_base::MissingBase),
                Box::new(unknown_plugin_command::UnknownPluginCommand),
                Box::new(plugin_conflict::PluginConflict),
                Box::new(vehicle_start_map::VehicleStartMap),
                Box::new(circular_gate::CircularGate),
                Box::new(picture_lifecycle::PictureLifecycle),
                Box::new(empty_event_page::EmptyEventPage),
                Box::new(blocked_tile::BlockedTile),
                Box::new(db_reachability::DbReachability),
            ],
        }
    }

    /// Empty registry (for tests and assembling a rule set piecewise).
    pub fn empty() -> Self {
        Self { rules: Vec::new() }
    }

    /// Adds a rule to the registry.
    pub fn register(&mut self, rule: Box<dyn Rule>) {
        self.rules.push(rule);
    }

    /// Identifiers of the registered rules.
    pub fn rule_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.rules.iter().map(|r| r.id())
    }

    /// Runs all rules and collects the findings (without sorting — that is done by
    /// [`crate::report::Report::new`]).
    pub fn run_all(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        self.rules.iter().flat_map(|r| r.run(ctx)).collect()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::with_builtin()
    }
}
