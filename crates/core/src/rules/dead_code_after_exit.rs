//! Rule `dead-code-after-exit`: unreachable code after an event exit.
//!
//! Within a single page's command list, an exit command (for RPG Maker — `115`,
//! the code is passed via [`RuleCtx::exit_command_codes`]) terminates processing.
//! Commands after it **at the same or greater indent** up to the first indent
//! decrease are unreachable (an indent decrease = closing the branch/block that
//! contained the exit — the path outside stays live).
//!
//! The core does not interpret code semantics: it only matches the exit number
//! supplied by the adapter, preserving engine independence.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::Entity;
use crate::message::Msg;
use crate::rules::{Rule, RuleCtx};

/// Rule for finding code after an exit command.
pub struct DeadCodeAfterExit;

impl Rule for DeadCodeAfterExit {
    fn id(&self) -> &'static str {
        "dead-code-after-exit"
    }

    fn category(&self) -> Category {
        Category::DeadCode
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        if ctx.exit_command_codes.is_empty() {
            return Vec::new();
        }
        let is_exit = |code: u16| ctx.exit_command_codes.contains(&code);
        let is_label = |code: u16| ctx.label_command_codes.contains(&code);

        let mut findings = Vec::new();
        for node in &ctx.ir.entities {
            let Entity::Page(page) = &node.kind else {
                continue;
            };
            let cmds = &page.commands;
            let mut i = 0;
            while i < cmds.len() {
                if !is_exit(cmds[i].code) {
                    i += 1;
                    continue;
                }
                let exit_indent = cmds[i].indent;
                // Scan the tail until the indent drops below the exit's indent.
                let mut j = i + 1;
                let mut flagged_any = false;
                while j < cmds.len() && cmds[j].indent >= exit_indent {
                    // A label (Jump-to-Label target) makes the code from here on
                    // potentially reachable via a jump that skips the exit — stop
                    // flagging rather than emit a Certain false positive.
                    if is_label(cmds[j].code) {
                        break;
                    }
                    findings.push(Finding {
                        severity: Severity::Warning,
                        category: Category::DeadCode,
                        confidence: Confidence::Certain,
                        location: cmds[j].location.clone(),
                        message: Msg::DeadCodeAfterExit { code: cmds[j].code },
                        references: vec![cmds[i].location.clone()],
                        rule: "dead-code-after-exit",
                    });
                    flagged_any = true;
                    j += 1;
                }
                // Continue after the processed tail.
                i = if flagged_any { j } else { i + 1 };
            }
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        CommandMeta, Engine, Entity, Ir, Location, Page, PageConditions, PageTrigger, PathSeg,
    };

    /// RPG Maker command codes for the test: 115 = Exit Event Processing.
    const EXIT: &[u16] = &[115];

    fn cmd(code: u16, indent: i32, index: u32) -> CommandMeta {
        CommandMeta {
            code,
            indent,
            index,
            location: Location::new(
                "data/Map001.json",
                vec![
                    PathSeg::Map(1),
                    PathSeg::Event(1),
                    PathSeg::Page(1),
                    PathSeg::Command(index),
                ],
            ),
        }
    }

    fn page_with(commands: Vec<CommandMeta>) -> Ir {
        let mut b = Ir::builder(Engine::Mz);
        b.push_entity(
            Entity::Page(Page {
                conditions: PageConditions::default(),
                trigger: PageTrigger::Action,
                command_count: commands.len() as u32,
                commands,
            }),
            Location::file_only("data/Map001.json"),
        );
        b.finish()
    }

    #[test]
    fn flags_commands_after_exit_at_same_indent() {
        // 0: 401 text; 1: 115 exit; 2: 101 (dead); 3: 250 (dead).
        let ir = page_with(vec![
            cmd(401, 0, 0),
            cmd(115, 0, 1),
            cmd(101, 0, 2),
            cmd(250, 0, 3),
        ]);
        let ctx = RuleCtx::with_exit_codes(&ir, EXIT);
        let f = DeadCodeAfterExit.run(&ctx);
        assert_eq!(f.len(), 2);
        assert!(f.iter().all(|x| x.severity == Severity::Warning));
        assert!(f.iter().all(|x| x.rule == "dead-code-after-exit"));
    }

    #[test]
    fn exit_inside_branch_does_not_kill_outer_code() {
        // Exit at indent 1 (inside a condition); a command at indent 0 after the branch is live.
        let ir = page_with(vec![
            cmd(111, 0, 0), // condition
            cmd(115, 1, 1), // exit inside the branch
            cmd(101, 0, 2), // after the branch (indent dropped) — LIVE
        ]);
        let ctx = RuleCtx::with_exit_codes(&ir, EXIT);
        let f = DeadCodeAfterExit.run(&ctx);
        assert!(f.is_empty(), "код вне ветки не должен помечаться мёртвым");
    }

    #[test]
    fn no_exit_codes_disables_rule() {
        let ir = page_with(vec![cmd(115, 0, 0), cmd(101, 0, 1)]);
        let ctx = RuleCtx::new(&ir); // exit_command_codes is empty
        assert!(DeadCodeAfterExit.run(&ctx).is_empty());
    }

    #[test]
    fn label_after_exit_stops_flagging() {
        // 0: 115 exit; 1: 101 (dead); 2: 118 Label (jump target); 3: 101 (reachable).
        // With label codes known, flagging stops at the label: only cmd 1 is dead.
        let ir = page_with(vec![
            cmd(115, 0, 0),
            cmd(101, 0, 1),
            cmd(118, 0, 2),
            cmd(101, 0, 3),
        ]);
        let ctx = RuleCtx::with_codes(&ir, EXIT, &[], &[118]);
        let f = DeadCodeAfterExit.run(&ctx);
        assert_eq!(f.len(), 1, "только код до метки помечается мёртвым");
        assert!(matches!(f[0].message, Msg::DeadCodeAfterExit { code: 101 }));
    }
}
