//! Rule `empty-event-page`: an unconditional Autorun/Parallel page that does
//! nothing.
//!
//! An **Autorun** page with no conditions and an empty command list freezes the
//! game: Autorun blocks input while active, and with nothing to run (and no
//! condition to clear) it stays active forever (soft-lock). A **Parallel** page
//! that is empty runs every frame but has no effect — most likely forgotten /
//! unfinished content.
//!
//! Only **unconditional** pages are flagged, so this does not overlap with
//! `stuck-autorun` (which handles *gated* autorun pages that fail to clear their
//! condition). Emptiness is judged against the adapter-supplied no-op codes
//! ([`RuleCtx::noop_command_codes`], RPG Maker `0` — the list terminator the editor
//! always appends), so a blank page (a single terminator) counts as empty.
//! Confidence `likely`: page selection across multiple pages is not modelled.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::{PageConditions, PageTrigger};
use crate::message::Msg;
use crate::rules::page_index::pages_by_event;
use crate::rules::{Rule, RuleCtx};

/// Rule that flags unconditional empty Autorun/Parallel pages.
pub struct EmptyEventPage;

/// Whether the page has no activation condition at all.
fn is_unconditional(c: &PageConditions) -> bool {
    c.switch1.is_none()
        && c.switch2.is_none()
        && c.variable.is_none()
        && c.self_switch.is_none()
        && c.item.is_none()
        && c.actor.is_none()
}

impl Rule for EmptyEventPage {
    fn id(&self) -> &'static str {
        "empty-event-page"
    }

    fn category(&self) -> Category {
        Category::Reference
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        for (_, pages) in pages_by_event(ctx.ir) {
            for page in &pages {
                let (severity, message) = match page.page.trigger {
                    PageTrigger::Autorun => (
                        Severity::Warning,
                        Msg::EmptyAutorunPage {
                            page: page.page_no,
                            event: page.event_id,
                        },
                    ),
                    PageTrigger::Parallel => (
                        Severity::Info,
                        Msg::EmptyParallelPage {
                            page: page.page_no,
                            event: page.event_id,
                        },
                    ),
                    _ => continue,
                };
                if !is_unconditional(&page.page.conditions) {
                    continue;
                }
                // Empty = every command is a no-op/terminator (the editor always
                // appends a trailing `0`). An `all` over an empty list is `true`.
                let empty = page
                    .page
                    .commands
                    .iter()
                    .all(|c| ctx.noop_command_codes.contains(&c.code));
                if !empty {
                    continue;
                }
                let location = ctx
                    .ir
                    .entity(page.entity)
                    .map(|n| n.location.clone())
                    .unwrap_or_else(|| crate::ir::Location::file_only(""));
                findings.push(Finding {
                    severity,
                    category: Category::Reference,
                    confidence: Confidence::Likely,
                    location,
                    message,
                    references: Vec::new(),
                    rule: "empty-event-page",
                });
            }
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{
        CommandMeta, Engine, Entity, EntityId, Ir, IrBuilder, Location, Page, PageConditions,
        PathSeg,
    };

    const NOOP: &[u16] = &[0];

    fn push_page(
        b: &mut IrBuilder,
        event: u32,
        no: u32,
        trigger: PageTrigger,
        cond: PageConditions,
        codes: &[u16],
    ) -> EntityId {
        let base = vec![PathSeg::Map(1), PathSeg::Event(event), PathSeg::Page(no)];
        let commands = codes
            .iter()
            .enumerate()
            .map(|(i, &code)| CommandMeta {
                code,
                indent: 0,
                index: i as u32,
                location: Location::file_only("data/Map001.json"),
            })
            .collect();
        b.push_entity(
            Entity::Page(Page {
                conditions: cond,
                trigger,
                command_count: codes.len() as u32,
                commands,
            }),
            Location::new("data/Map001.json", base),
        )
    }

    fn ctx_with_noop(ir: &Ir) -> RuleCtx<'_> {
        RuleCtx::with_codes(ir, &[], &[], &[]).with_noop_codes(NOOP)
    }

    #[test]
    fn flags_unconditional_empty_autorun() {
        let mut b = Ir::builder(Engine::Mz);
        // Blank page = a single terminator (code 0).
        push_page(
            &mut b,
            5,
            1,
            PageTrigger::Autorun,
            PageConditions::default(),
            &[0],
        );
        let ir = b.finish();
        let f = EmptyEventPage.run(&ctx_with_noop(&ir));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Warning);
        assert!(matches!(
            f[0].message,
            Msg::EmptyAutorunPage { page: 1, event: 5 }
        ));
    }

    #[test]
    fn flags_unconditional_empty_parallel_as_info() {
        let mut b = Ir::builder(Engine::Mz);
        push_page(
            &mut b,
            6,
            1,
            PageTrigger::Parallel,
            PageConditions::default(),
            &[],
        );
        let ir = b.finish();
        let f = EmptyEventPage.run(&ctx_with_noop(&ir));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Info);
        assert!(matches!(
            f[0].message,
            Msg::EmptyParallelPage { page: 1, event: 6 }
        ));
    }

    #[test]
    fn spares_gated_autorun() {
        // A gated autorun is stuck-autorun's domain, not this rule's.
        let mut b = Ir::builder(Engine::Mz);
        push_page(
            &mut b,
            5,
            1,
            PageTrigger::Autorun,
            PageConditions {
                self_switch: Some('A'),
                ..Default::default()
            },
            &[0],
        );
        let ir = b.finish();
        assert!(EmptyEventPage.run(&ctx_with_noop(&ir)).is_empty());
    }

    #[test]
    fn spares_nonempty_autorun_and_action_pages() {
        let mut b = Ir::builder(Engine::Mz);
        // Non-empty autorun (has a real command 121).
        push_page(
            &mut b,
            5,
            1,
            PageTrigger::Autorun,
            PageConditions::default(),
            &[121, 0],
        );
        // Empty Action page — not autorun/parallel, ignored (new events default here).
        push_page(
            &mut b,
            6,
            1,
            PageTrigger::Action,
            PageConditions::default(),
            &[0],
        );
        let ir = b.finish();
        assert!(EmptyEventPage.run(&ctx_with_noop(&ir)).is_empty());
    }
}
