//! Integration test: running the analyzer on the synthetic MZ fixture
//! `testdata/mz-fixture/`. Verifies that every planned rule fires
//! exactly where the bug was planted (see `PLANTED-BUGS.md`).

use dk_doctor_core::{Finding, Lang, Msg, Registry, RuleCtx, SymbolKind, render};
use dk_doctor_rpgmaker::load_project;

/// Command code for «Exit Event Processing» (as in the CLI).
const EXIT_CODES: &[u16] = &[115];

/// Codes for an «untraceable exit» used by `stuck-autorun` (as in the CLI).
const OPAQUE_CODES: &[u16] = &[117, 355, 356, 357];

/// Command code for the «Label» marker (as in the CLI).
const LABEL_CODES: &[u16] = &[118];

/// Command code for the «empty command» block terminator (as in the CLI).
const NOOP_CODES: &[u16] = &[0];

/// Loads the fixture and runs all built-in rules.
fn run_fixture() -> Vec<Finding> {
    let root = camino::Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("testdata")
        .join("mz-fixture");
    let ir = load_project(&root).expect("фикстура должна загрузиться");
    assert_eq!(ir.engine, dk_doctor_core::Engine::Mz, "движок = MZ");
    let registry = Registry::with_builtin();
    let ctx =
        RuleCtx::with_codes(&ir, EXIT_CODES, OPAQUE_CODES, LABEL_CODES).with_noop_codes(NOOP_CODES);
    registry.run_all(&ctx)
}

/// Findings with the given rule id.
fn by_rule<'a>(findings: &'a [Finding], rule: &str) -> Vec<&'a Finding> {
    findings.iter().filter(|f| f.rule == rule).collect()
}

#[test]
fn each_planted_rule_fires_exactly() {
    let findings = run_fixture();

    // 1. dead-variables: #2 DeadCounter.
    let dead = by_rule(&findings, "dead-variables");
    assert_eq!(dead.len(), 1, "ровно одна мёртвая переменная");
    assert!(matches!(
        &dead[0].message,
        Msg::DeadVariable { id: 2, name: Some(n), .. } if n == "DeadCounter"
    ));

    // 2. uninitialized-symbols: switch #2 BossDefeated. Plugins parsed
    //    (plugins.js present) ⇒ the remainder is checked against @param: confidence Certain,
    //    plugin_checked=true.
    let uninit = by_rule(&findings, "uninitialized-symbols");
    assert_eq!(uninit.len(), 1, "ровно один неинициализированный символ");
    assert!(matches!(
        &uninit[0].message,
        Msg::UninitializedSymbol {
            kind: SymbolKind::Switch, id: 2, name: Some(n), plugin_checked: true, ..
        } if n == "BossDefeated"
    ));
    assert_eq!(
        uninit[0].confidence,
        dk_doctor_core::Confidence::Certain,
        "сверено с плагинами ⇒ Certain"
    );

    // 3. broken-transfer: -> map #99.
    let broken_tr = by_rule(&findings, "broken-transfer");
    assert_eq!(broken_tr.len(), 1, "ровно один битый переход");
    assert!(matches!(
        broken_tr[0].message,
        Msg::BrokenTransfer { map_id: 99 }
    ));

    // 4. unreachable-maps: map #3 SecretRoom.
    let unreach = by_rule(&findings, "unreachable-maps");
    assert_eq!(unreach.len(), 1, "ровно одна недостижимая карта");
    assert!(matches!(
        &unreach[0].message,
        Msg::UnreachableMap { map_id: 3, name } if name == "SecretRoom"
    ));

    // 5. referential-integrity: 5 dangling DB references.
    //    - item #99 (command 126, Map002)              — original bug
    //    - tileset #99 (Map003.tilesetId)              — map DB FK
    //    - skill #99 (Class #1 learnings)              — class DB FK
    //    - item #99 (Enemy #1 dropItems)               — enemy DB FK
    //    - commonEvent #99 (Item #1 effect 44)         — effect DB FK
    let refint = by_rule(&findings, "referential-integrity");
    assert_eq!(refint.len(), 5, "пять висячих ссылок БД");
    let has = |k: dk_doctor_core::DbKind, id: u32| {
        refint.iter().any(
            |f| matches!(f.message, Msg::DanglingDbRef { kind, id: i } if kind == k && i == id),
        )
    };
    assert!(has(dk_doctor_core::DbKind::Item, 99), "item #99");
    assert!(has(dk_doctor_core::DbKind::Tileset, 99), "tileset #99");
    assert!(has(dk_doctor_core::DbKind::Skill, 99), "skill #99");
    assert!(
        has(dk_doctor_core::DbKind::CommonEvent, 99),
        "commonEvent #99 (эффект 44)"
    );

    // 6. broken-assets: GhostPic (231 Show Picture) + MissingArena (battleback
    //    of Map002, which HAS an encounterList → can_battle → gets checked) +
    //    Outside_B (tileset #1 image slot, missing on disk; tileset #1 is USED by
    //    Map001/Map002 → usage-gated IN).
    //    usage-gating controls NOT flagged: NoBattleHere — battleback of Map003
    //    with no battle (no encounters / 301).
    let broken_asset = by_rule(&findings, "broken-assets");
    let broken_names: Vec<&str> = broken_asset
        .iter()
        .filter_map(|f| match &f.message {
            Msg::BrokenAsset { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(broken_asset.len(), 3, "три битых ассета");
    assert!(broken_names.contains(&"GhostPic"), "GhostPic битый");
    assert!(
        broken_names.contains(&"MissingArena"),
        "MissingArena (battleback карты с боем) битый"
    );
    assert!(
        broken_names.contains(&"Outside_B"),
        "Outside_B (тайлсет используемой карты, картинки нет) битый"
    );
    assert!(
        !broken_names.contains(&"NoBattleHere"),
        "NoBattleHere (battleback карты без боя) НЕ должен флагаться (usage-gating)"
    );

    // 7. orphan-assets: Unused (while Slime/Actor1/Explosion are NOT orphans).
    let orphans = by_rule(&findings, "orphan-assets");
    assert_eq!(orphans.len(), 1, "ровно один сирота-ассет");
    assert!(matches!(
        &orphans[0].message,
        Msg::OrphanAsset { name, .. } if name == "Unused"
    ));

    // 8. dead-code-after-exit: real commands after 115 (101 + 108). The trailing
    //    code:0 list terminator is a structural no-op and must NOT be flagged.
    let dead_code = by_rule(&findings, "dead-code-after-exit");
    assert_eq!(dead_code.len(), 2, "две недостижимые команды после Exit");
    assert!(
        dead_code
            .iter()
            .any(|f| matches!(f.message, Msg::DeadCodeAfterExit { code: 101 }))
    );
    assert!(
        dead_code
            .iter()
            .any(|f| matches!(f.message, Msg::DeadCodeAfterExit { code: 108 }))
    );

    // 9. dead-self-switch: EV003 "SwitchLogic" — 123 sets 'B', never read anywhere.
    //    'C' is both written AND read (page condition) → control, not flagged.
    let dead_ss = by_rule(&findings, "dead-self-switch");
    assert_eq!(dead_ss.len(), 1, "ровно один мёртвый self-switch");
    assert!(matches!(
        dead_ss[0].message,
        Msg::DeadSelfSwitch { ch: 'B', event: 3 }
    ));

    // 10. unreachable-self-switch: EV003 page2 requires 'D', which nobody sets.
    let unreach_ss = by_rule(&findings, "unreachable-self-switch");
    assert_eq!(
        unreach_ss.len(),
        1,
        "ровно один недостижимый по self-switch"
    );
    assert!(matches!(
        unreach_ss[0].message,
        Msg::UnreachableSelfSwitch { ch: 'D', event: 3 }
    ));

    // 11. dead-common-event: CE #2 "Orphan" (trigger None, no calls).
    //     CE #1 "Init" (Autorun) and CE #3/#4 (call each other) — not flagged.
    let dead_ce = by_rule(&findings, "dead-common-event");
    assert_eq!(dead_ce.len(), 1, "ровно одно мёртвое общее событие");
    assert!(matches!(
        &dead_ce[0].message,
        Msg::DeadCommonEvent { id: 2, name } if name == "Orphan"
    ));

    // 12. cyclic-common-events: CE #3 ↔ CE #4 (mutual 117).
    let cyclic = by_rule(&findings, "cyclic-common-events");
    assert_eq!(cyclic.len(), 1, "ровно один цикл общих событий");
    assert!(matches!(
        &cyclic[0].message,
        Msg::CyclicCommonEvents { cycle } if cycle == &vec![3, 4]
    ));

    // 13. shadowed-page: EV001 "Greeter" page1 (requires switch1+switch2)
    //     shadowed by page2 (no conditions → always wins).
    let shadowed = by_rule(&findings, "shadowed-page");
    assert_eq!(shadowed.len(), 1, "ровно одна перекрытая страница");
    assert!(matches!(
        shadowed[0].message,
        Msg::ShadowedPage {
            page: 1,
            by_page: 2,
            event: 1
        }
    ));

    // 14. stuck-autorun: Map002 EV002 "Loop" — an Autorun page, gated on
    //     switch #1, shows text and does NOTHING to exit (no write of a
    //     self-switch / switch / transfer) → soft-lock.
    //     Control 1: Map002 EV003 "GoodLoop" — Autorun gated on switch #1, but
    //     writes self-switch 'A' (123) → legitimate pattern, NOT flagged.
    //     Control 2 (Tier A): Map002 EV004 "PluginLoop" — Autorun gated on
    //     switch #3 PluginGate, also with no exit, BUT switch #3 is declared by the
    //     GateCore plugin (@type switch) → the plugin clears the page at runtime →
    //     SUPPRESSED, does not appear in the findings.
    let stuck = by_rule(&findings, "stuck-autorun");
    assert_eq!(
        stuck.len(),
        1,
        "ровно один soft-lock Autorun (EV004 на plugin-switch подавлен)"
    );
    assert!(matches!(
        stuck[0].message,
        Msg::StuckAutorun { page: 1, event: 2 }
    ));

    // 15. unknown-plugin-command: 357 call ("DummyPlugin","doNothing") — plugin
    //     not installed (no plugins.js in the fixture), command not registered.
    let unknown = by_rule(&findings, "unknown-plugin-command");
    assert_eq!(unknown.len(), 1, "ровно одна неизвестная плагин-команда");
    assert!(matches!(
        &unknown[0].message,
        Msg::UnknownPluginCommand { plugin: Some(p), command, structured: true, plugin_loaded: false }
            if p == "DummyPlugin" && command == "doNothing"
    ));

    // 16. plugin-load-order: DependentPlugin declares @orderAfter LatePlugin
    //     (LatePlugin must load EARLIER), but in plugins.js LatePlugin is placed
    //     AFTER DependentPlugin → order violated.
    let load_order = by_rule(&findings, "plugin-load-order");
    assert_eq!(load_order.len(), 1, "ровно одно нарушение порядка загрузки");
    assert!(matches!(
        &load_order[0].message,
        Msg::PluginLoadOrder { plugin, dependency, tag: dk_doctor_core::PluginOrderTag::OrderAfter }
            if plugin == "DependentPlugin" && dependency == "LatePlugin"
    ));

    // 17. missing-base: DependentPlugin declares @base GhostBase, which is not
    //     in plugins.js (not disabled, but genuinely absent) → disabled:false.
    //     Control: DisabledPlugin (status:false) is NOT in load_order, its @command
    //     "unused" is not registered, the file is not read.
    let missing_base = by_rule(&findings, "missing-base");
    assert_eq!(missing_base.len(), 1, "ровно одна отсутствующая база");
    assert!(matches!(
        &missing_base[0].message,
        Msg::MissingBase { plugin, base, disabled: false }
            if plugin == "DependentPlugin" && base == "GhostBase"
    ));

    // 18. impossible-condition: Map001 EV002 page1 — var #1 (Gold) assigned 10
    //     by command 122, then condition 111 «Gold == 5» → always false → the then-branch
    //     (writing DeadCounter) is unreachable. Light constant-propagation.
    let impossible = by_rule(&findings, "impossible-condition");
    assert_eq!(
        impossible.len(),
        1,
        "ровно одно константно-разрешимое условие"
    );
    assert!(matches!(
        &impossible[0].message,
        Msg::ImpossibleCondition {
            var_id: 1,
            value: 10,
            operand: 5,
            result: false,
            ..
        }
    ));
    assert_eq!(impossible[0].confidence, dk_doctor_core::Confidence::Likely);
}

/// Entity name appearing in the message (for control checks).
fn msg_name(msg: &Msg) -> Option<&str> {
    match msg {
        Msg::DeadVariable { name, .. } | Msg::UninitializedSymbol { name, .. } => name.as_deref(),
        Msg::UnreachableMap { name, .. }
        | Msg::BrokenAsset { name, .. }
        | Msg::OrphanAsset { name, .. } => Some(name),
        _ => None,
    }
}

#[test]
fn control_cases_not_flagged() {
    let findings = run_fixture();

    // Control: switch #1 DoorOpened and var #1 Gold must not appear in the findings.
    assert!(
        !findings
            .iter()
            .any(|f| msg_name(&f.message) == Some("DoorOpened")),
        "DoorOpened (пишется+читается) не должен флагаться"
    );
    assert!(
        !findings
            .iter()
            .any(|f| msg_name(&f.message) == Some("Gold")),
        "Gold (пишется+читается) не должен флагаться"
    );
    // Control: control assets are neither orphans nor broken.
    for present in ["Actor1", "Slime", "Explosion", "Potion"] {
        assert!(
            !findings
                .iter()
                .any(|f| (f.rule == "broken-assets" || f.rule == "orphan-assets")
                    && msg_name(&f.message) == Some(present)),
            "{present} не должен быть битым/сиротой"
        );
    }
}

#[test]
fn fixture_message_renders_in_both_languages() {
    let findings = run_fixture();
    // Take a specific named finding (the dead variable DeadCounter).
    let dead = by_rule(&findings, "dead-variables");
    let msg = &dead[0].message;
    let ru = render(msg, Lang::Ru);
    let en = render(msg, Lang::En);
    assert_ne!(ru, en, "русский и английский тексты должны отличаться");
    // The variable name is language-neutral and present in both languages.
    assert!(ru.contains("DeadCounter"));
    assert!(en.contains("DeadCounter"));
    // Tokens characteristic of each language.
    assert!(ru.contains("мёртвый стейт"));
    assert!(en.contains("dead state"));
}

#[test]
fn summary_counts_match() {
    let findings = run_fixture();
    let report = dk_doctor_core::Report::new(findings);
    // Errors: referential-integrity ×5 + broken-transfer + broken-assets ×3
    //         (GhostPic + MissingArena + Outside_B) + missing-base + plugin-load-order.
    assert_eq!(report.summary.errors, 11, "11 ошибок");
    // Warnings: uninitialized-symbols + dead-variables +
    // dead-code-after-exit ×2 + dead-self-switch + unreachable-self-switch +
    // cyclic-common-events + shadowed-page + stuck-autorun +
    // unknown-plugin-command + impossible-condition.
    assert_eq!(report.summary.warnings, 11, "11 предупреждений");
    // Info: unreachable-maps + orphan-assets + dead-common-event.
    assert_eq!(report.summary.infos, 3, "3 информационных");
    assert_eq!(report.exit_code(), 2, "код выхода 2 (есть ошибки)");
}
