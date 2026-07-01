//! Remediation metadata for a finding: a short "why it matters" line, a concrete
//! "how to fix it" line, and a stable documentation link.
//!
//! Like [`crate::message`], the data is decoupled from its text: remediation is a
//! **pure function of the typed [`Msg`]** (plus the language), computed at render
//! time rather than stored on every [`crate::Finding`]. This keeps the rules free
//! of presentation and lets the CLI/desktop emit remediation without threading an
//! extra field through 25 rules. The catalog uses the same **exhaustive `match`**
//! convention as the message catalog — a new `Msg` variant is a compile error, not
//! a silent gap.
//!
//! A small subset of findings also carries a machine-applicable [`Fix`]
//! ([`autofix`]) — for now only the case-only asset rename, the one edit that is
//! safe to apply blindly (it changes casing, never meaning).

use crate::message::{Lang, Msg};

/// Base URL of the per-rule documentation. Each rule id is a heading anchor in
/// `docs/rules.md`, so `<base>#<rule-id>` deep-links to that rule's section.
const DOCS_BASE: &str = "https://github.com/DKPlugins/DK-Doctor/blob/main/docs/rules.md";

/// Remediation metadata attached to a finding at render time.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Remediation {
    /// One-line "why this matters" (the impact), rendered in the chosen language.
    pub why: String,
    /// One-line concrete fix (the action), rendered in the chosen language.
    pub suggested_fix: String,
    /// Stable deep link to the rule's documentation section (language-neutral).
    pub docs_url: String,
}

/// A machine-applicable, safe-to-auto-apply fix.
///
/// Only emitted where the edit cannot change meaning. Today that is exclusively
/// the case-only asset rename (align the reference casing with the on-disk file).
#[derive(Clone, Debug, serde::Serialize)]
pub struct Fix {
    /// Kind of fix (selects how a consumer applies it).
    pub kind: FixKind,
    /// The current (offending) text.
    pub from: String,
    /// The corrected text to replace it with.
    pub to: String,
}

/// Kind of [`Fix`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FixKind {
    /// Align an asset reference's letter case with the on-disk file name.
    AssetCaseRename,
}

/// Builds the remediation metadata for a finding message in the chosen language.
pub fn remediation(msg: &Msg, lang: Lang) -> Remediation {
    let (why, suggested_fix) = match lang {
        Lang::Ru => remediation_ru(msg),
        Lang::En => remediation_en(msg),
    };
    Remediation {
        why: why.to_string(),
        suggested_fix: suggested_fix.to_string(),
        docs_url: format!("{DOCS_BASE}#{}", docs_slug(msg)),
    }
}

/// Returns a safe machine-applicable fix for the finding, if one exists.
///
/// Currently only [`Msg::AssetCaseMismatch`] qualifies: the reference and the
/// on-disk file differ solely in case, so replacing the referenced name with the
/// on-disk one is a meaning-preserving edit.
pub fn autofix(msg: &Msg) -> Option<Fix> {
    match msg {
        Msg::AssetCaseMismatch {
            referenced,
            on_disk,
            ..
        } => Some(Fix {
            kind: FixKind::AssetCaseRename,
            from: referenced.clone(),
            to: on_disk.clone(),
        }),
        _ => None,
    }
}

/// Documentation anchor for a message — the id of the rule that emits it.
///
/// Exhaustive so a new `Msg` variant must declare where it is documented.
fn docs_slug(msg: &Msg) -> &'static str {
    match msg {
        Msg::DeadVariable { .. } => "dead-variables",
        Msg::UninitializedSymbol { .. } => "uninitialized-symbols",
        Msg::BrokenTransfer { .. } | Msg::BrokenTransferVar { .. } => "broken-transfer",
        Msg::VehicleStartMapMissing { .. } => "vehicle-start-map",
        Msg::UnreachableMap { .. } => "unreachable-maps",
        Msg::DanglingDbRef { .. } => "referential-integrity",
        Msg::BrokenAsset { .. } | Msg::AssetCaseMismatch { .. } => "broken-assets",
        Msg::OrphanAsset { .. } => "orphan-assets",
        Msg::DeadCodeAfterExit { .. } => "dead-code-after-exit",
        Msg::DeadSelfSwitch { .. } => "dead-self-switch",
        Msg::UnreachableSelfSwitch { .. } => "unreachable-self-switch",
        Msg::DeadCommonEvent { .. } => "dead-common-event",
        Msg::CyclicCommonEvents { .. } => "cyclic-common-events",
        Msg::ShadowedPage { .. } => "shadowed-page",
        Msg::StuckAutorun { .. } => "stuck-autorun",
        Msg::PluginLoadOrder { .. } => "plugin-load-order",
        Msg::MissingBase { .. } => "missing-base",
        Msg::UnknownPluginCommand { .. } => "unknown-plugin-command",
        Msg::PluginConflict { .. } => "plugin-conflict",
        Msg::ImpossibleCondition { .. } => "impossible-condition",
        Msg::CircularGate { .. } => "circular-gate",
        Msg::TransferToBlockedTile { .. } | Msg::StartInWall { .. } => "blocked-tile",
        Msg::PictureBeforeShow { .. } => "picture-lifecycle",
        Msg::EmptyAutorunPage { .. } | Msg::EmptyParallelPage { .. } => "empty-event-page",
        Msg::UnusedDbRecord { .. } => "db-reachability",
    }
}

/// Russian remediation catalog: `(why, suggested_fix)` per message variant.
fn remediation_ru(msg: &Msg) -> (&'static str, &'static str) {
    match msg {
        Msg::DeadVariable { .. } => (
            "Запись в переменную, которую никто не читает, — потерянное состояние: либо \
             забытое чтение, либо опечатка в id.",
            "Уберите запись или добавьте недостающее чтение; проверьте, что id переменной верный.",
        ),
        Msg::UninitializedSymbol { .. } => (
            "Символ читается, но нигде не записывается — значение остаётся стартовым, и условие \
             может никогда не сработать.",
            "Запишите переключатель/переменную до чтения или исправьте id, если запись идёт в другой.",
        ),
        Msg::BrokenTransfer { .. } => (
            "Переход на несуществующую карту роняет игру в момент срабатывания события.",
            "Направьте переход на существующую карту или создайте недостающую.",
        ),
        Msg::BrokenTransferVar { .. } => (
            "Переход по переменной на несуществующую карту роняет игру, если переменная придёт с \
             этим значением.",
            "Проверьте диапазон значений переменной и создайте недостающую карту или исправьте цель.",
        ),
        Msg::VehicleStartMapMissing { .. } => (
            "Стартовая карта транспорта отсутствует — он не появится, и в него нельзя сесть.",
            "Укажите существующую стартовую карту или переставляйте транспорт командой \
             «Set Vehicle Location».",
        ),
        Msg::UnreachableMap { .. } => (
            "Карта недостижима прямыми переходами от старта — возможно, забытый контент.",
            "Добавьте переход на неё или убедитесь, что её открывает плагин/переход по переменной.",
        ),
        Msg::DanglingDbRef { .. } => (
            "Ссылка на несуществующую запись базы данных упадёт в рантайме.",
            "Создайте запись или перенаправьте ссылку на существующий id.",
        ),
        Msg::BrokenAsset { .. } => (
            "Ссылка на отсутствующий файл не загрузится — чёрная картинка или тишина.",
            "Добавьте файл в папку или исправьте имя в ссылке.",
        ),
        Msg::AssetCaseMismatch { .. } => (
            "Файл есть, но с другим регистром букв: на Windows/macOS загрузится, а на \
             регистрозависимых системах (серверы Linux, веб-сборки) — нет.",
            "Приведите регистр ссылки к имени файла на диске (или переименуйте файл) — только буквы.",
        ),
        Msg::OrphanAsset { .. } => (
            "Файл, на который нет ссылок, раздувает сборку — либо остаток, либо грузится плагином.",
            "Удалите неиспользуемый файл или убедитесь, что его загружает плагин.",
        ),
        Msg::DeadCodeAfterExit { .. } => (
            "Команды после выхода из события никогда не выполнятся.",
            "Уберите недостижимые команды или перенесите команду выхода.",
        ),
        Msg::DeadSelfSwitch { .. } => (
            "Self-switch выставляется, но нигде не проверяется — запись ни на что не влияет.",
            "Добавьте условие страницы на этот self-switch или уберите запись.",
        ),
        Msg::UnreachableSelfSwitch { .. } => (
            "Страница требует self-switch, который ничто не выставляет, — она не активируется.",
            "Добавьте команду, выставляющую self-switch, или ослабьте условие страницы.",
        ),
        Msg::DeadCommonEvent { .. } => (
            "Общее событие без триггера и без входящих вызовов никогда не запускается.",
            "Задайте триггер, вызовите его из события или удалите.",
        ),
        Msg::CyclicCommonEvents { .. } => (
            "Общие события, вызывающие друг друга по кругу, уходят в бесконечную рекурсию и вешают игру.",
            "Разорвите цикл: добавьте условие по переключателю/переменной или перестройте вызовы.",
        ),
        Msg::ShadowedPage { .. } => (
            "Более поздняя страница с более слабыми условиями всегда перекрывает эту — она не запустится.",
            "Ужесточите условия поздней страницы либо переставьте/уберите перекрытую.",
        ),
        Msg::StuckAutorun { .. } => (
            "Автозапуск, который сам себя не выключает, блокирует ввод — soft-lock.",
            "Выключайте условие (self-switch/switch) внутри страницы или смените триггер.",
        ),
        Msg::PluginLoadOrder { .. } => (
            "Плагин грузится раньше зависимости и может инициализироваться против недостающего кода.",
            "Переставьте плагины в Plugin Manager согласно объявленному порядку.",
        ),
        Msg::MissingBase { .. } => (
            "Объявленный @base отсутствует или выключен — плагин не получит нужный код и упадёт при загрузке.",
            "Добавьте или включите базовый плагин.",
        ),
        Msg::UnknownPluginCommand { .. } => (
            "Плагин-команду, которую не регистрирует ни один включённый плагин, движок молча пропустит.",
            "Исправьте имя команды либо включите/добавьте плагин, который её предоставляет.",
        ),
        Msg::PluginConflict { .. } => (
            "Два плагина перетирают один метод без alias — поздний по порядку молча выигрывает.",
            "Проверьте совместимость и порядок загрузки или используйте патч, сохраняющий оригинал (alias).",
        ),
        Msg::ImpossibleCondition { .. } => (
            "Сравнение с заранее известным результатом делает одну из веток мёртвым кодом.",
            "Исправьте значение переменной или само сравнение либо уберите недостижимую ветку.",
        ),
        Msg::CircularGate { .. } => (
            "Переключатели, взаимно закрывающие друг друга, никогда не включатся — контент за ними недостижим.",
            "Разорвите зависимость: дайте хотя бы одному переключателю включаться извне связки.",
        ),
        Msg::TransferToBlockedTile { .. } => (
            "Переход ставит игрока на клетку, непроходимую со всех сторон, — с неё не сойти (soft-lock).",
            "Перенесите цель на проходимую клетку или поправьте флаги проходимости тайлсета.",
        ),
        Msg::StartInWall { .. } => (
            "Игрок стартует на непроходимой клетке и не может двигаться.",
            "Перенесите стартовую позицию на проходимую клетку.",
        ),
        Msg::PictureBeforeShow { .. } => (
            "Операция над картинкой до её показа (Show Picture) ни на что не влияет.",
            "Сначала покажите картинку (Show Picture) или переставьте команды.",
        ),
        Msg::EmptyAutorunPage { .. } => (
            "Пустая страница Autorun бесконечно блокирует ввод — soft-lock.",
            "Добавьте команды/условие или смените триггер с Autorun.",
        ),
        Msg::EmptyParallelPage { .. } => (
            "Пустая Parallel-страница крутится каждый кадр вхолостую — похоже на забытый контент.",
            "Впишите нужные команды или удалите страницу.",
        ),
        Msg::UnusedDbRecord { .. } => (
            "Запись базы данных, на которую нет ссылок, — вероятно, неиспользуемый контент.",
            "Сошлитесь на неё там, где задумано, или удалите (ссылки из плагинов/заметок не отслеживаются).",
        ),
    }
}

/// English remediation catalog: `(why, suggested_fix)` per message variant.
fn remediation_en(msg: &Msg) -> (&'static str, &'static str) {
    match msg {
        Msg::DeadVariable { .. } => (
            "A variable written but never read is lost state — a forgotten read or a typo in the id.",
            "Remove the write, or add the intended read; check the variable id.",
        ),
        Msg::UninitializedSymbol { .. } => (
            "A symbol read but never written stays at its default, so the condition may never fire.",
            "Set the switch/variable before reading it, or fix the id if the write targets another.",
        ),
        Msg::BrokenTransfer { .. } => (
            "A transfer to a nonexistent map crashes the game when the event fires.",
            "Point the transfer at an existing map, or create the missing one.",
        ),
        Msg::BrokenTransferVar { .. } => (
            "A variable transfer to a nonexistent map crashes the game if the variable arrives with that value.",
            "Check the variable's value range and create the missing map or fix the destination.",
        ),
        Msg::VehicleStartMapMissing { .. } => (
            "The vehicle's start map is missing, so it never spawns and cannot be boarded.",
            "Set an existing start map, or reposition the vehicle with a Set Vehicle Location command.",
        ),
        Msg::UnreachableMap { .. } => (
            "A map unreachable by direct transfers from the start is possibly orphaned content.",
            "Add a transfer to it, or confirm a plugin/variable transfer opens it.",
        ),
        Msg::DanglingDbRef { .. } => (
            "A reference to a database record that does not exist will fail at runtime.",
            "Create the record, or repoint the reference to an existing id.",
        ),
        Msg::BrokenAsset { .. } => (
            "A reference to a file missing from disk will not load — a black image or silent audio.",
            "Add the file to the folder, or fix the referenced name.",
        ),
        Msg::AssetCaseMismatch { .. } => (
            "The file exists but under different letter case: it loads on Windows/macOS but not on \
             case-sensitive systems (Linux servers, web builds).",
            "Align the reference casing with the on-disk file name (or rename the file) — letters only.",
        ),
        Msg::OrphanAsset { .. } => (
            "A file nothing references bloats the build — leftover, or loaded only by a plugin.",
            "Delete it if unused, or confirm a plugin loads it.",
        ),
        Msg::DeadCodeAfterExit { .. } => (
            "Commands after an event exit never run.",
            "Remove the unreachable commands, or move the exit command.",
        ),
        Msg::DeadSelfSwitch { .. } => (
            "A self switch set but never checked has no effect.",
            "Add a page condition on the self switch, or remove the write.",
        ),
        Msg::UnreachableSelfSwitch { .. } => (
            "A page requires a self switch that nothing ever sets, so it can never activate.",
            "Add a command that sets the self switch, or relax the page condition.",
        ),
        Msg::DeadCommonEvent { .. } => (
            "A common event with no trigger and no incoming caller never runs.",
            "Give it a trigger, call it from an event, or delete it.",
        ),
        Msg::CyclicCommonEvents { .. } => (
            "Common events calling each other in a cycle recurse infinitely and freeze the game.",
            "Break the cycle: guard with a switch/variable, or restructure the calls.",
        ),
        Msg::ShadowedPage { .. } => (
            "A later page with looser conditions always wins over this one, so it never runs.",
            "Tighten the later page's conditions, or reorder/remove the shadowed page.",
        ),
        Msg::StuckAutorun { .. } => (
            "An Autorun page that never turns itself off blocks all input — a soft-lock.",
            "Turn the gating switch/self-switch off inside the page, or change the trigger.",
        ),
        Msg::PluginLoadOrder { .. } => (
            "A plugin loading before its dependency may initialize against missing code.",
            "Reorder the plugins in Plugin Manager to satisfy the declared order.",
        ),
        Msg::MissingBase { .. } => (
            "A declared @base is missing or disabled, so the plugin lacks required code and likely crashes on load.",
            "Add or enable the base plugin.",
        ),
        Msg::UnknownPluginCommand { .. } => (
            "A plugin command that no enabled plugin registers is silently skipped by the engine.",
            "Fix the command name, or enable/add the plugin that provides it.",
        ),
        Msg::PluginConflict { .. } => (
            "Two plugins overwrite the same method without an alias — the later one silently wins.",
            "Check compatibility and load order, or use a patch that keeps the original (alias).",
        ),
        Msg::ImpossibleCondition { .. } => (
            "A comparison whose result is fixed makes one branch dead code.",
            "Correct the variable value or the comparison, or remove the dead branch.",
        ),
        Msg::CircularGate { .. } => (
            "Switches that mutually gate each other can never turn on — the content behind them is unreachable.",
            "Break the dependency so at least one switch can be set from outside the cluster.",
        ),
        Msg::TransferToBlockedTile { .. } => (
            "A transfer lands the player on a tile blocked on all sides — they cannot move off it (soft-lock).",
            "Move the destination to a passable tile, or adjust the tileset passability flags.",
        ),
        Msg::StartInWall { .. } => (
            "The player starts on an impassable tile and cannot move.",
            "Move the start position to a passable tile.",
        ),
        Msg::PictureBeforeShow { .. } => (
            "Operating a picture before it is shown (Show Picture) has no effect.",
            "Show the picture first, or reorder the commands.",
        ),
        Msg::EmptyAutorunPage { .. } => (
            "An empty Autorun page blocks input forever — a soft-lock.",
            "Add commands/a condition, or change the trigger away from Autorun.",
        ),
        Msg::EmptyParallelPage { .. } => (
            "An empty Parallel page runs every frame doing nothing — likely forgotten content.",
            "Fill in the intended commands, or remove the page.",
        ),
        Msg::UnusedDbRecord { .. } => (
            "A database record referenced nowhere is likely unused content.",
            "Reference it where intended, or remove it (plugin/notetag references are not tracked).",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::DbKind;

    #[test]
    fn remediation_is_populated_and_language_specific() {
        let msg = Msg::BrokenTransfer { map_id: 42 };
        let ru = remediation(&msg, Lang::Ru);
        let en = remediation(&msg, Lang::En);
        assert!(!ru.why.is_empty() && !ru.suggested_fix.is_empty());
        assert!(!en.why.is_empty() && !en.suggested_fix.is_empty());
        assert_ne!(ru.why, en.why);
        // docs_url is language-neutral and anchored on the rule id.
        assert_eq!(ru.docs_url, en.docs_url);
        assert!(ru.docs_url.ends_with("#broken-transfer"));
    }

    #[test]
    fn docs_url_uses_the_emitting_rule_slug() {
        let msg = Msg::UnusedDbRecord {
            kind: DbKind::Enemy,
            id: 3,
            name: None,
        };
        assert!(
            remediation(&msg, Lang::En)
                .docs_url
                .ends_with("#db-reachability")
        );
    }

    #[test]
    fn autofix_only_for_case_mismatch() {
        let case = Msg::AssetCaseMismatch {
            folder: "img/pictures".to_string(),
            referenced: "Hero".to_string(),
            on_disk: "hero".to_string(),
        };
        let fix = autofix(&case).expect("case mismatch has an autofix");
        assert_eq!(fix.kind, FixKind::AssetCaseRename);
        assert_eq!(fix.from, "Hero");
        assert_eq!(fix.to, "hero");
        // A plain broken asset has no safe autofix (the file genuinely does not exist).
        let broken = Msg::BrokenAsset {
            folder: "img/pictures".to_string(),
            name: "Ghost".to_string(),
        };
        assert!(autofix(&broken).is_none());
    }
}
