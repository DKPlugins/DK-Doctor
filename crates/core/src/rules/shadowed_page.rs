//! Rule `shadowed-page`: a lower event page is unreachable because a later page
//! with weaker conditions always wins.
//!
//! RPG Maker picks the page with the **highest** index among those whose
//! conditions are satisfied (`Game_Event.findProperPageIndex` scans from the end
//! of the list). Therefore a lower page `L` is unreachable if there exists a page
//! `H` with a higher index whose set of required predicates is a **subset** of
//! `L`'s predicates: whenever `L` is active, `H` is also active and shadows it.
//!
//! We flag only **gated** lower pages: if a lower page has no conditions at all,
//! its "shadowing" by a later page is trivial (usually a duplicate event body
//! with a different trigger, Event Touch + Player Touch), and there is no
//! interesting reachability bug here. A condition is mandatory.
//!
//! Conservative (better to miss than to produce a false positive):
//! - switch1/switch2/self-switch/item/actor — equality of id (the predicate
//!   "enabled"/"present"); set `H` ⊆ set `L`.
//! - variable — only if it is the same variable AND threshold `H` ≤ threshold `L`
//!   (comparison `>=`): then condition `L` implies condition `H`. Otherwise we
//!   don't flag.
//! - empty conditions `H` shadow any lower page.

use crate::finding::{Category, Confidence, Finding, Severity};
use crate::ir::PageConditions;
use crate::message::Msg;
use crate::rules::page_index::{PageRef, pages_by_event};
use crate::rules::{Rule, RuleCtx};

/// Rule for finding shadowed (unreachable) lower pages.
pub struct ShadowedPage;

/// Whether the page has at least one gating condition (switch/variable/self-switch/
/// item/actor). A page with no conditions is always active — its "shadowing" by a
/// later page is trivial (usually a duplicate with a different trigger:
/// Event Touch + Player Touch on the same body) and is NOT an interesting
/// reachability bug. So we flag only lower pages that actually have a condition
/// that is lost due to shadowing.
fn has_condition(c: &PageConditions) -> bool {
    c.switch1.is_some()
        || c.switch2.is_some()
        || c.variable.is_some()
        || c.self_switch.is_some()
        || c.item.is_some()
        || c.actor.is_some()
}

/// Whether condition `outer` (of the upper page) holds whenever condition `inner`
/// (of the lower page) holds — i.e. the set of required predicates of `outer` is a
/// subset of the predicates of `inner`.
fn outer_is_implied_by_inner(outer: &PageConditions, inner: &PageConditions) -> bool {
    // Global "enabled" switches: every id required by `outer` must also be
    // required by `inner` (switch1 and switch2 share one "enabled" space).
    let inner_switches = [inner.switch1, inner.switch2];
    for req in [outer.switch1, outer.switch2].into_iter().flatten() {
        if !inner_switches.contains(&Some(req)) {
            return false;
        }
    }
    // self-switch, item, actor — equality of the required channel/id.
    if let Some(ch) = outer.self_switch
        && inner.self_switch != Some(ch)
    {
        return false;
    }
    if let Some(it) = outer.item
        && inner.item != Some(it)
    {
        return false;
    }
    if let Some(ac) = outer.actor
        && inner.actor != Some(ac)
    {
        return false;
    }
    // Variable: only the same variable with threshold outer ≤ threshold inner.
    if let Some(ov) = outer.variable {
        match (inner.variable, outer.variable_value, inner.variable_value) {
            (Some(iv), Some(othr), Some(ithr)) if iv == ov && othr <= ithr => {}
            _ => return false,
        }
    }
    true
}

impl Rule for ShadowedPage {
    fn id(&self) -> &'static str {
        "shadowed-page"
    }

    fn category(&self) -> Category {
        Category::Reference
    }

    fn run(&self, ctx: &RuleCtx<'_>) -> Vec<Finding> {
        let mut findings = Vec::new();
        for (_, pages) in pages_by_event(ctx.ir) {
            if pages.len() < 2 {
                continue;
            }
            // For each lower page L we look for an upper page H that shadows it.
            for li in 0..pages.len() {
                let lower: &PageRef = &pages[li];
                // A lower page with no conditions is a trivial duplicate, not a bug.
                if !has_condition(&lower.page.conditions) {
                    continue;
                }
                for higher in &pages[li + 1..] {
                    if outer_is_implied_by_inner(&higher.page.conditions, &lower.page.conditions) {
                        let location = ctx
                            .ir
                            .entity(lower.entity)
                            .map(|n| n.location.clone())
                            .unwrap_or_else(|| crate::ir::Location::file_only(""));
                        let references = ctx
                            .ir
                            .entity(higher.entity)
                            .map(|n| vec![n.location.clone()])
                            .unwrap_or_default();
                        findings.push(Finding {
                            severity: Severity::Warning,
                            category: Category::Reference,
                            confidence: Confidence::Likely,
                            location,
                            message: Msg::ShadowedPage {
                                page: lower.page_no,
                                by_page: higher.page_no,
                                event: lower.event_id,
                            },
                            references,
                            rule: "shadowed-page",
                        });
                        // One shadowing page is enough — we stop searching further.
                        break;
                    }
                }
            }
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Engine, Entity, Ir, Location, Page, PageConditions, PageTrigger, PathSeg};

    fn page_entity(b: &mut IrBuilderShim, map: u32, event: u32, no: u32, cond: PageConditions) {
        let base = vec![PathSeg::Map(map), PathSeg::Event(event), PathSeg::Page(no)];
        b.0.push_entity(
            Entity::Page(Page {
                conditions: cond,
                trigger: PageTrigger::Action,
                command_count: 0,
                commands: vec![],
            }),
            Location::new("data/Map001.json", base),
        );
    }

    struct IrBuilderShim(crate::ir::IrBuilder);

    fn cond_switch(id: u32) -> PageConditions {
        PageConditions {
            switch1: Some(id),
            ..Default::default()
        }
    }

    #[test]
    fn flags_lower_page_shadowed_by_unconditional_higher() {
        let mut b = IrBuilderShim(Ir::builder(Engine::Mz));
        // page1: requires switch 10. page2: no conditions → shadows page1.
        page_entity(&mut b, 1, 5, 1, cond_switch(10));
        page_entity(&mut b, 1, 5, 2, PageConditions::default());
        let ir = b.0.finish();
        let f = ShadowedPage.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].rule, "shadowed-page");
        assert_eq!(f[0].severity, Severity::Warning);
        assert_eq!(f[0].confidence, Confidence::Likely);
        assert!(matches!(
            f[0].message,
            Msg::ShadowedPage {
                page: 1,
                by_page: 2,
                event: 5
            }
        ));
    }

    #[test]
    fn spares_higher_page_with_stricter_conditions() {
        let mut b = IrBuilderShim(Ir::builder(Engine::Mz));
        // page1: no conditions. page2: requires switch 10 (stricter) → page1 is NOT shadowed.
        page_entity(&mut b, 1, 5, 1, PageConditions::default());
        page_entity(&mut b, 1, 5, 2, cond_switch(10));
        let ir = b.0.finish();
        let f = ShadowedPage.run(&RuleCtx::new(&ir));
        assert!(f.is_empty());
    }

    #[test]
    fn spares_unconditional_lower_page_trivial_duplicate() {
        // page1 and page2 are both unconditional, differing only by trigger (a typical
        // Event Touch + Player Touch duplicate). The shadowing is trivial, not a bug.
        let mut b = IrBuilderShim(Ir::builder(Engine::Mz));
        page_entity(&mut b, 1, 5, 1, PageConditions::default());
        page_entity(&mut b, 1, 5, 2, PageConditions::default());
        let ir = b.0.finish();
        assert!(ShadowedPage.run(&RuleCtx::new(&ir)).is_empty());
    }

    #[test]
    fn spares_disjoint_switch_conditions() {
        let mut b = IrBuilderShim(Ir::builder(Engine::Mz));
        // page1: switch 10, page2: switch 20 — the sets are not nested → no shadowing.
        page_entity(&mut b, 1, 5, 1, cond_switch(10));
        page_entity(&mut b, 1, 5, 2, cond_switch(20));
        let ir = b.0.finish();
        let f = ShadowedPage.run(&RuleCtx::new(&ir));
        assert!(f.is_empty());
    }

    #[test]
    fn variable_threshold_subset_flagged_but_stricter_spared() {
        // H requires var3>=5, L requires var3>=10: always L⇒H → shadowing.
        let mut b = IrBuilderShim(Ir::builder(Engine::Mz));
        let lower = PageConditions {
            variable: Some(3),
            variable_value: Some(10),
            ..Default::default()
        };
        let higher = PageConditions {
            variable: Some(3),
            variable_value: Some(5),
            ..Default::default()
        };
        page_entity(&mut b, 1, 5, 1, lower);
        page_entity(&mut b, 1, 5, 2, higher);
        let ir = b.0.finish();
        let f = ShadowedPage.run(&RuleCtx::new(&ir));
        assert_eq!(f.len(), 1);

        // Reversed threshold order (H stricter than L) → we don't flag.
        let mut b2 = IrBuilderShim(Ir::builder(Engine::Mz));
        let lower2 = PageConditions {
            variable: Some(3),
            variable_value: Some(5),
            ..Default::default()
        };
        let higher2 = PageConditions {
            variable: Some(3),
            variable_value: Some(10),
            ..Default::default()
        };
        page_entity(&mut b2, 1, 5, 1, lower2);
        page_entity(&mut b2, 1, 5, 2, higher2);
        let ir2 = b2.0.finish();
        assert!(ShadowedPage.run(&RuleCtx::new(&ir2)).is_empty());
    }
}
