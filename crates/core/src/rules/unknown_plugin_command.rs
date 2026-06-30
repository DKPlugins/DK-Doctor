//! Rule `unknown-plugin-command`: a call to a plugin command (356/357) that is
//! absent from the `@command` registry of every enabled plugin.
//!
//! The registry is built by the adapter from the `@command` annotations of enabled
//! plugins ([`crate::ir::PluginMeta::commands`]). The adapter collects 356/357 calls
//! while traversing events ([`crate::ir::Ir::plugin_command_calls`]).
//!
//! - **357 (MZ)** gives the `(plugin, command)` pair structurally: we match exactly
//!   by the pair → confidence `Certain`. A mismatch = typo / disabled /
//!   missing plugin: the command will not run.
//! - **357 to a LOADED plugin**: in Tier A this was not flagged at all (many
//!   MZ plugins register commands in JS via `PluginManager.registerCommand`,
//!   which the annotations don't see — otherwise it floods, Haven produced 2080). **Tier B** parses
//!   `registerCommand` from the plugin's code and assembles the full registry. If the plugin is in
//!   `command_registry_known` (its JS was parsed, the registration is literal/complete) and
//!   the command is NOT among the known ones — this is a typo in the command name (`likely`). If
//!   the plugin's registry is incomplete/untraced — we do NOT flag (conservatively).
//! - **356 (MV)** — a raw string `"PluginName arg..."`: the plugin/command names are not
//!   separated, registration goes through `Game_Interpreter.pluginCommand`. We can't match
//!   it against the registry (a continuous flood) — it is not checked.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};
use rustc_hash::FxHashSet;

/// Rule that matches plugin command calls against the `@command`/`registerCommand` registry.
pub struct UnknownPluginCommand;

impl Rule for UnknownPluginCommand {
    fn id(&self) -> &'static str {
        "unknown-plugin-command"
    }

    fn category(&self) -> Category {
        Category::PluginConflict
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let meta = &ctx.ir.plugin_meta;
        // Set of known (plugin, command) pairs from the registry.
        let registered: FxHashSet<(&str, &str)> = meta
            .commands
            .iter()
            .map(|c| (c.plugin.as_str(), c.command.as_str()))
            .collect();
        let mut findings = Vec::new();
        for (call, location) in &ctx.ir.plugin_command_calls {
            // We match ONLY the structured 357 (MZ); raw 356 (MV) is not checked.
            if !call.structured {
                continue;
            }
            let Some(plugin) = &call.plugin else {
                continue;
            };
            let loaded = meta.load_order.iter().any(|n| n == plugin);
            let (severity, confidence) = if !loaded {
                // A call to a MISSING/disabled plugin — the command is guaranteed
                // not to run (Certain).
                (Severity::Warning, Confidence::Certain)
            } else {
                // Loaded plugin: we flag a command typo ONLY if its
                // registry is fully known (Tier B) and the command is not in it.
                let registry_known = meta.command_registry_known.iter().any(|n| n == plugin);
                if !registry_known {
                    continue;
                }
                if registered.contains(&(plugin.as_str(), call.command.as_str())) {
                    continue;
                }
                (Severity::Warning, Confidence::Likely)
            };
            findings.push(Finding {
                severity,
                category: Category::PluginConflict,
                confidence,
                location: location.clone(),
                message: Msg::UnknownPluginCommand {
                    plugin: Some(plugin.clone()),
                    command: call.command.clone(),
                    structured: call.structured,
                    plugin_loaded: loaded,
                },
                references: Vec::new(),
                rule: "unknown-plugin-command",
            });
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Engine, Ir, Location, PluginCommand, PluginCommandCall, PluginMeta};

    /// IR builder with the given loaded plugins, known registries, and calls.
    fn ir_full(
        loaded: &[&str],
        registry_known: &[&str],
        commands: &[(&str, &str)],
        calls: Vec<PluginCommandCall>,
    ) -> Ir {
        let mut b = Ir::builder(Engine::Mz);
        let mut meta = PluginMeta::new();
        meta.load_order = loaded.iter().map(|s| s.to_string()).collect();
        meta.command_registry_known = registry_known.iter().map(|s| s.to_string()).collect();
        meta.commands = commands
            .iter()
            .map(|(p, c)| PluginCommand {
                plugin: p.to_string(),
                command: c.to_string(),
            })
            .collect();
        b.set_plugin_meta(meta);
        for call in calls {
            b.add_plugin_command_call(call, Location::file_only("data/Map001.json"));
        }
        b.finish()
    }

    fn ir_with(loaded: &[&str], calls: Vec<PluginCommandCall>) -> Ir {
        ir_full(loaded, &[], &[], calls)
    }

    fn call_357(plugin: &str, command: &str) -> PluginCommandCall {
        PluginCommandCall {
            plugin: Some(plugin.to_string()),
            command: command.to_string(),
            structured: true,
        }
    }

    fn call_356(command: &str) -> PluginCommandCall {
        PluginCommandCall {
            plugin: None,
            command: command.to_string(),
            structured: false,
        }
    }

    #[test]
    fn flags_call_to_unloaded_plugin() {
        // The "Missing" plugin is absent from plugins.js → its command will not run.
        let ir = ir_with(&["Loaded"], vec![call_357("Missing", "doThing")]);
        let f = UnknownPluginCommand.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].confidence, Confidence::Certain);
        assert_eq!(f[0].severity, Severity::Warning);
        assert!(matches!(
            &f[0].message,
            Msg::UnknownPluginCommand { plugin: Some(p), command, plugin_loaded: false, .. }
                if p == "Missing" && command == "doThing"
        ));
    }

    #[test]
    fn accepts_loaded_plugin_with_unknown_registry() {
        // The plugin is loaded, but its registry was not parsed (registry_known is empty) —
        // we can't claim a typo, so we don't flag (Haven anti-flood).
        let ir = ir_with(&["Core"], vec![call_357("Core", "anyCommand")]);
        assert!(UnknownPluginCommand.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn accepts_registered_command_of_known_plugin() {
        // Registry is known, the command is registered — fine.
        let ir = ir_full(
            &["Core"],
            &["Core"],
            &[("Core", "doThing")],
            vec![call_357("Core", "doThing")],
        );
        assert!(UnknownPluginCommand.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn flags_typo_command_of_known_plugin() {
        // Core's registry is known and contains doThing; doThng was called → typo (likely).
        let ir = ir_full(
            &["Core"],
            &["Core"],
            &[("Core", "doThing")],
            vec![call_357("Core", "doThng")],
        );
        let f = UnknownPluginCommand.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].confidence, Confidence::Likely);
        assert!(matches!(
            &f[0].message,
            Msg::UnknownPluginCommand { plugin: Some(p), command, plugin_loaded: true, .. }
                if p == "Core" && command == "doThng"
        ));
    }

    #[test]
    fn never_flags_356_raw_mv() {
        // raw 356 (MV) is not matched: commands are registered in JS pluginCommand.
        let ir = ir_with(&["Core"], vec![call_356("whatever")]);
        assert!(UnknownPluginCommand.run(&RuleCtx::new(&ir)).is_empty());
    }
}
