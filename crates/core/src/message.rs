//! Multilingual report messages: message identity is decoupled from its text.
//!
//! Rules and rendering construct only [`Msg`]/[`Chrome`] (typed data, without
//! ready-made text). The human-readable text lives exclusively in the
//! [`render`]/[`render_chrome`] catalog — one template per language. This
//! allows: (1) serializing a message as language-neutral data in JSON,
//! (2) rendering it in the chosen language without duplicating logic in rules.
//!
//! The catalog uses an **exhaustive** `match` over [`Lang`] and [`Msg`] — with
//! no `_ =>` fallback, so a missing translation is caught by the compiler.

use crate::ir::{CmpOp, DbKind, VehicleKind};

/// Report output language.
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    /// Russian.
    Ru,
    /// English.
    En,
}

/// Symbol-table kind (for [`Msg::UninitializedSymbol`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    /// Switch.
    Switch,
    /// Variable.
    Variable,
}

/// Finding message: a typed identity without ready-made text.
///
/// One variant per distinguishable message. Arguments are typed (numbers,
/// optional names); formatting happens only in the catalog.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "key", rename_all = "snake_case")]
pub enum Msg {
    /// A variable is written but never read (dead state).
    DeadVariable {
        /// Numeric variable id.
        id: u32,
        /// Variable name from the database, if set.
        name: Option<String>,
        /// How many times the variable is written.
        writes: usize,
    },
    /// A switch/var is read but never written (uninitialized).
    UninitializedSymbol {
        /// Symbol kind.
        kind: SymbolKind,
        /// Numeric symbol id.
        id: u32,
        /// Symbol name from the database, if set.
        name: Option<String>,
        /// How many times the symbol is read.
        reads: usize,
        /// Whether it was cross-checked against plugin `@param` (Tier A parsed
        /// the plugins and the symbol is NOT declared by any of them). If
        /// `false`, plugins were not parsed, so we add the disclaimer that the
        /// value could have been set by a plugin.
        plugin_checked: bool,
    },
    /// A Transfer Player to a nonexistent map.
    BrokenTransfer {
        /// Id of the target map that does not exist in the project.
        map_id: u32,
    },
    /// A variable Transfer Player (designation 1) to a nonexistent map: the map
    /// id was obtained by light constant propagation (the variable was assigned
    /// a literal value earlier in the same command list).
    BrokenTransferVar {
        /// Id of the target map that does not exist in the project.
        map_id: u32,
    },
    /// The start map of a vehicle (boat/ship/airship) does not exist.
    VehicleStartMapMissing {
        /// Vehicle type.
        vehicle: VehicleKind,
        /// Id of the missing start map.
        map_id: u32,
    },
    /// A map is unreachable via direct transfers from the start map.
    UnreachableMap {
        /// Id of the unreachable map.
        map_id: u32,
        /// Map name.
        name: String,
    },
    /// A reference to a database entry that does not exist (dangling reference).
    DanglingDbRef {
        /// Kind of database entry (rendered as a label, e.g. "Items").
        kind: DbKind,
        /// Id of the missing entry.
        id: u32,
    },
    /// A reference to an asset that is not on disk.
    BrokenAsset {
        /// Asset folder (e.g. `img/pictures`).
        folder: String,
        /// Asset file name (without extension).
        name: String,
    },
    /// An asset on disk that nothing references (possibly unused).
    OrphanAsset {
        /// Asset folder (e.g. `img/pictures`).
        folder: String,
        /// Asset file name.
        name: String,
    },
    /// An unreachable command after exiting the event.
    DeadCodeAfterExit {
        /// Numeric code of the unreachable command.
        code: u16,
    },
    /// A self-switch is written but never checked (dead self-switch).
    DeadSelfSwitch {
        /// Self-switch channel (`'A'..'D'`).
        ch: char,
        /// Id of the event the self-switch belongs to.
        event: u32,
    },
    /// A page condition requires a self-switch that nobody sets.
    UnreachableSelfSwitch {
        /// Self-switch channel required by the condition (`'A'..'D'`).
        ch: char,
        /// Id of the event whose page is unreachable.
        event: u32,
    },
    /// A common event with no trigger and no incoming callers — never runs.
    DeadCommonEvent {
        /// Common-event id.
        id: u32,
        /// Common-event name.
        name: String,
    },
    /// A cycle in mutual common-event calls (command 117) — infinite recursion.
    CyclicCommonEvents {
        /// Ids of the common events forming the cycle (in call order).
        cycle: Vec<u32>,
    },
    /// A page is shadowed by a later page with looser conditions.
    ShadowedPage {
        /// Number of the unreachable (lower) page (1-based).
        page: u32,
        /// Number of the shadowing (upper) page (1-based).
        by_page: u32,
        /// Id of the event the pages belong to.
        event: u32,
    },
    /// An Autorun page that cannot turn itself off — soft-lock
    /// (freeze: Autorun blocks input while active).
    StuckAutorun {
        /// Page number (1-based).
        page: u32,
        /// Id of the event the page belongs to.
        event: u32,
    },
    /// A plugin loads before a dependency it is supposed to load after.
    PluginLoadOrder {
        /// The plugin whose order declaration is violated.
        plugin: String,
        /// The dependency plugin that ended up in the wrong position.
        dependency: String,
        /// Source of the requirement: `@base` / `@orderAfter` / `@orderBefore`.
        tag: PluginOrderTag,
    },
    /// `@base <X>` where X is missing from plugins.js or disabled.
    MissingBase {
        /// The plugin that declared the dependency.
        plugin: String,
        /// Name of the missing/disabled base plugin.
        base: String,
        /// The base plugin is present but disabled (`true`), or missing entirely.
        disabled: bool,
    },
    /// A plugin-command call that is not in the registry of any enabled plugin.
    UnknownPluginCommand {
        /// Plugin name from the 357 call (`None` for a raw 356 MV call).
        plugin: Option<String>,
        /// Command name.
        command: String,
        /// Structured 357 call (`true`) or raw 356 MV best-effort (`false`).
        structured: bool,
        /// The plugin IS LOADED (in plugins.js), but no such command is among
        /// its commands (the known `@command` + `registerCommand` registry,
        /// Tier B) → a typo in the command name. If `false`, the plugin is
        /// missing/disabled (the command will not run at all).
        plugin_loaded: bool,
    },
    /// A constant-resolvable condition (command 111): the variable was assigned
    /// a literal value earlier in the list, so the comparison always yields the
    /// same result → one of the branches is unreachable (dead code).
    ImpossibleCondition {
        /// Id of the variable in the condition.
        var_id: u32,
        /// The propagated constant value of the variable.
        value: i64,
        /// Comparison operator.
        op: CmpOp,
        /// Right-hand operand of the comparison.
        operand: i64,
        /// What the condition is guaranteed to evaluate to (`true` → the "else"
        /// branch is dead; `false` → the "then" branch is dead).
        result: bool,
    },
    /// A single core method is patched by >=2 enabled plugins — load order
    /// decides, and the later one may silently clobber the earlier one's logic
    /// (Tier B AST heuristic).
    PluginConflict {
        /// The patched method (e.g. `Game_Battler.prototype.gainHp`).
        method: String,
        /// All patching plugins in load order.
        plugins: Vec<String>,
        /// The subset that overwrites the method (assigns without keeping the
        /// original implementation in an alias) — these are the ones that lose
        /// another plugin's logic.
        overwriters: Vec<String>,
    },
}

/// Source of the load-order requirement (for [`Msg::PluginLoadOrder`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginOrderTag {
    /// `@base` — a hard dependency (must load earlier).
    Base,
    /// `@orderAfter` — must load after the specified plugin.
    OrderAfter,
    /// `@orderBefore` — must load before the specified plugin.
    OrderBefore,
}

/// Render "chrome" — service strings around the findings (not the findings themselves).
#[derive(Clone, Debug)]
pub enum Chrome {
    /// Heading of the "ERRORS" section.
    HeadingError,
    /// Heading of the "WARNINGS" section.
    HeadingWarning,
    /// Heading of the "INFO" section.
    HeadingInfo,
    /// "No problems found.".
    NoProblems,
    /// Prefix of the summary line ("Итог:" / "Total:").
    SummaryPrefix,
    /// Summary line with counters.
    Summary {
        /// Number of errors.
        errors: usize,
        /// Number of warnings.
        warnings: usize,
        /// Number of info findings.
        infos: usize,
    },
    /// Prefix of the hint line ("Подсказка:" / "Hint:").
    HintPrefix,
    /// Prefix of the related-sites list ("связано:").
    Related,
    /// `likely` confidence tag.
    TagLikely,
    /// Hint about `--orphans`.
    OrphansHint,
    /// Hint about `--dead-common-events`.
    DeadCommonEventsHint,
    /// Note: N findings were suppressed by the project config (`.dk-doctor.toml`).
    SuppressedNote {
        /// Number of findings hidden by `[[suppress]]` entries.
        count: usize,
    },
    /// Note: a fresh baseline was written with N fingerprints.
    BaselineWritten {
        /// Number of fingerprints written.
        count: usize,
    },
    /// Note: N findings are new relative to the baseline (`--fail-on new`).
    NewFindingsNote {
        /// Number of findings absent from the baseline.
        count: usize,
    },
    /// Header of the "files skipped" note: N project files could not be parsed.
    ParseWarningsHeader {
        /// Number of project files that could not be parsed and were skipped.
        count: usize,
    },
    /// Report header: "— project analysis (engine: …)".
    Header {
        /// Engine label (`mv`/`mz`).
        engine: String,
    },
    /// Project load error (for CLI/desktop) — localized by kind.
    LoadError {
        /// Load error kind (selects the localized text).
        kind: LoadErrorKind,
        /// Detail (path / file name / system message) — language-neutral.
        detail: String,
    },
}

/// Project load error kind — a UI taxonomy, engine-independent.
///
/// The adapter maps its own errors into these kinds, and the catalog renders
/// user-friendly text in the chosen language (instead of the raw error text).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LoadErrorKind {
    /// The folder does not look like a project: no `data/`.
    NotFound,
    /// The data is unreadable: encrypted or not RPG Maker.
    NotAnalyzable,
    /// I/O error while reading files.
    Io,
    /// Failed to parse a project file.
    ParseError,
}

/// Database entry name in the chosen language (for [`Msg::DanglingDbRef`]).
fn db_kind_label(kind: DbKind, lang: Lang) -> &'static str {
    match lang {
        Lang::Ru => match kind {
            DbKind::Actor => "актёр",
            DbKind::Class => "класс",
            DbKind::Skill => "навык",
            DbKind::Item => "предмет",
            DbKind::Weapon => "оружие",
            DbKind::Armor => "броня",
            DbKind::Enemy => "враг",
            DbKind::Troop => "группа врагов",
            DbKind::State => "состояние",
            DbKind::Animation => "анимация",
            DbKind::Tileset => "тайлсет",
            DbKind::CommonEvent => "общее событие",
        },
        Lang::En => match kind {
            DbKind::Actor => "actor",
            DbKind::Class => "class",
            DbKind::Skill => "skill",
            DbKind::Item => "item",
            DbKind::Weapon => "weapon",
            DbKind::Armor => "armor",
            DbKind::Enemy => "enemy",
            DbKind::Troop => "troop",
            DbKind::State => "state",
            DbKind::Animation => "animation",
            DbKind::Tileset => "tileset",
            DbKind::CommonEvent => "common event",
        },
    }
}

/// Symbol kind name in the chosen language (for [`Msg::UninitializedSymbol`]).
fn symbol_kind_label(kind: SymbolKind, lang: Lang) -> &'static str {
    match lang {
        Lang::Ru => match kind {
            SymbolKind::Switch => "Переключатель",
            SymbolKind::Variable => "Переменная",
        },
        Lang::En => match kind {
            SymbolKind::Switch => "Switch",
            SymbolKind::Variable => "Variable",
        },
    }
}

/// Vehicle name in the chosen language (for [`Msg::VehicleStartMapMissing`]).
fn vehicle_label(v: VehicleKind, lang: Lang) -> &'static str {
    match lang {
        Lang::Ru => match v {
            VehicleKind::Boat => "лодки",
            VehicleKind::Ship => "корабля",
            VehicleKind::Airship => "дирижабля",
        },
        Lang::En => match v {
            VehicleKind::Boat => "boat",
            VehicleKind::Ship => "ship",
            VehicleKind::Airship => "airship",
        },
    }
}

/// Optional name insertion in guillemets / quotation marks.
fn name_label(name: &Option<String>, lang: Lang) -> String {
    match name.as_deref().filter(|n| !n.is_empty()) {
        Some(n) => match lang {
            Lang::Ru => format!(" «{n}»"),
            Lang::En => format!(" \"{n}\""),
        },
        None => String::new(),
    }
}

/// Renders a finding message in the chosen language.
///
/// An exhaustive `match` over [`Lang`] and [`Msg`]: a missing translation is a
/// compile error, not a silent fallback.
pub fn render(msg: &Msg, lang: Lang) -> String {
    match lang {
        Lang::Ru => render_ru(msg),
        Lang::En => render_en(msg),
    }
}

/// Russian message templates.
fn render_ru(msg: &Msg) -> String {
    match msg {
        Msg::DeadVariable { id, name, writes } => {
            let label = name_label(name, Lang::Ru);
            format!(
                "Переменная #{id}{label} записывается ({writes} раз), но нигде не читается — \
                 мёртвый стейт: запись ни на что не влияет."
            )
        }
        Msg::UninitializedSymbol {
            kind,
            id,
            name,
            reads,
            plugin_checked,
        } => {
            let label = name_label(name, Lang::Ru);
            let kind = symbol_kind_label(*kind, Lang::Ru);
            let caveat = if *plugin_checked {
                " Сверено с @param плагинов: символ не объявлен ни одним включённым плагином."
            } else {
                " Не сверено с @param плагинов (plugins.js не разобран): значение могло \
                 задаваться плагином."
            };
            format!(
                "{kind} #{id}{label} читается ({reads} раз), но нигде не записывается — \
                 условие может никогда не сработать / значение остаётся стартовым.{caveat}"
            )
        }
        Msg::BrokenTransfer { map_id } => {
            format!(
                "Переход (Transfer Player) на карту #{map_id}, которой нет в проекте — \
                 при срабатывании события игра упадёт."
            )
        }
        Msg::BrokenTransferVar { map_id } => {
            format!(
                "Переход по переменной (Transfer Player) на карту #{map_id}, которой нет в \
                 проекте: переменной присвоено это значение ранее в том же событии \
                 (constant-propagation). При срабатывании с таким значением игра упадёт. \
                 Достоверность likely: значение могло быть перезаписано вне статики."
            )
        }
        Msg::VehicleStartMapMissing { vehicle, map_id } => {
            format!(
                "Стартовая карта {} (System.json) указывает на карту #{map_id}, которой нет в \
                 проекте — транспорт не появится и в него нельзя будет сесть (мёртвый контент). \
                 Если это намеренно, транспорт переставляется командой «Set Vehicle Location» \
                 (202) в событии.",
                vehicle_label(*vehicle, Lang::Ru)
            )
        }
        Msg::UnreachableMap { map_id, name } => {
            format!(
                "Карта #{map_id} «{name}» недостижима прямыми переходами от стартовой карты — \
                 возможно, она открывается переходом по переменной, плагином или общим \
                 событием (это пока не отслеживается)."
            )
        }
        Msg::DanglingDbRef { kind, id } => {
            format!(
                "Ссылка на {} #{id} ({}), которого нет в базе данных — висячая ссылка.",
                db_kind_label(*kind, Lang::Ru),
                kind.file_stem()
            )
        }
        Msg::BrokenAsset { folder, name } => {
            format!("Ссылка на ассет «{name}» в {folder}/, которого нет на диске — не загрузится.")
        }
        Msg::OrphanAsset { folder, name } => {
            format!(
                "Файл «{name}» в {folder}/ присутствует, но на него нет ссылок в данных — \
                 возможно, не используется (ссылки из плагинов пока не учитываются)."
            )
        }
        Msg::DeadCodeAfterExit { code } => {
            format!(
                "Недостижимая команда (код {code}) после выхода из события — \
                 выполнение сюда не дойдёт."
            )
        }
        Msg::DeadSelfSwitch { ch, event } => {
            format!(
                "Self-switch {ch} события #{event} выставляется, но нигде не проверяется — \
                 мёртвый self-switch: запись ни на что не влияет."
            )
        }
        Msg::UnreachableSelfSwitch { ch, event } => {
            format!(
                "Условие страницы требует self-switch {ch}, который ни одна команда события \
                 #{event} не выставляет — страница недостижима. Self-switch могут выставлять \
                 плагины/скрипты ($gameSelfSwitches), что пока не отслеживается."
            )
        }
        Msg::DeadCommonEvent { id, name } => {
            let label = name_label(&Some(name.clone()), Lang::Ru);
            format!(
                "Общее событие #{id}{label} без триггера и без входящих вызовов (команда 117 \
                 или эффект 44) — никогда не запускается. Может резервироваться плагином/скриптом \
                 ($gameTemp.reserveCommonEvent), что пока не отслеживается."
            )
        }
        Msg::CyclicCommonEvents { cycle } => {
            let chain = render_cycle(cycle);
            format!(
                "Цикл во взаимных вызовах общих событий (команда 117): {chain} — \
                 бесконечная синхронная рекурсия при срабатывании."
            )
        }
        Msg::ShadowedPage {
            page,
            by_page,
            event,
        } => {
            format!(
                "Страница {page} события #{event} недостижима: более поздняя страница {by_page} \
                 (выше по индексу, с более слабыми условиями) выполняется всегда, когда активна \
                 страница {page}, и перекрывает её — RPG Maker берёт страницу с наибольшим \
                 индексом среди подходящих."
            )
        }
        Msg::StuckAutorun { page, event } => {
            format!(
                "Автозапускаемая (Autorun) страница {page} события #{event} включена условием, но не \
                 делает ничего, что могло бы её выключить (не пишет self-switch/switch, не переносит \
                 игрока, не вызывает общее событие/плагин-команду/скрипт) — Autorun блокирует ввод, \
                 пока активен, поэтому игра зависает (soft-lock). Учтены и исключены: switch/var, \
                 объявленные плагинами (@type) или записываемые их JS ($gameSwitches.setValue); \
                 gating-switch, который где-то выключается (121 OFF) или нигде не включается; \
                 страницы со скриптом/плагин-командой/вызовом общего события. Остаются неучтёнными \
                 вычисляемые id — достоверность likely."
            )
        }
        Msg::PluginLoadOrder {
            plugin,
            dependency,
            tag,
        } => {
            let req = match tag {
                PluginOrderTag::Base => format!(
                    "@base «{dependency}» (плагин «{plugin}» зависит от него и обязан грузиться позже)"
                ),
                PluginOrderTag::OrderAfter => format!(
                    "@orderAfter «{dependency}» (плагин «{plugin}» обязан грузиться после «{dependency}»)"
                ),
                PluginOrderTag::OrderBefore => format!(
                    "@orderBefore «{dependency}» (плагин «{plugin}» обязан грузиться до «{dependency}»)"
                ),
            };
            format!(
                "Нарушен порядок загрузки плагинов: {req}, но в plugins.js порядок обратный — \
                 плагин может инициализироваться раньше зависимости и упасть/работать неверно. \
                 Переставьте плагины в Plugin Manager."
            )
        }
        Msg::MissingBase {
            plugin,
            base,
            disabled,
        } => {
            let state = if *disabled {
                "присутствует, но выключен"
            } else {
                "отсутствует в plugins.js"
            };
            format!(
                "Плагин «{plugin}» объявляет @base «{base}», но базовый плагин {state} — \
                 «{plugin}» не получит требуемый код и, скорее всего, упадёт при загрузке. \
                 Добавьте/включите «{base}»."
            )
        }
        Msg::UnknownPluginCommand {
            plugin,
            command,
            structured,
            plugin_loaded,
        } => {
            let who = match plugin {
                Some(p) => format!("плагина «{p}» команда «{command}»"),
                None => format!("команда «{command}»"),
            };
            if *plugin_loaded {
                format!(
                    "Вызов плагин-команды: {who} не зарегистрирована этим (подключённым) \
                     плагином — ни через @command, ни через PluginManager.registerCommand. \
                     Похоже на опечатку в имени команды: вызов молча не выполнится."
                )
            } else if *structured {
                format!(
                    "Вызов плагин-команды: {who} не зарегистрирована ни одним включённым \
                     плагином — опечатка, отключённый или отсутствующий плагин: команда не \
                     выполнится."
                )
            } else {
                format!(
                    "Вызов плагин-команды (MV): {who} не найдена среди @command включённых \
                     плагинов — возможна опечатка или отключённый плагин. MV не разделяет имя \
                     плагина и команды, поэтому сверка приблизительная."
                )
            }
        }
        Msg::ImpossibleCondition {
            var_id,
            value,
            op,
            operand,
            result,
        } => {
            let verdict = if *result {
                "всегда истинно"
            } else {
                "всегда ложно"
            };
            let dead = if *result {
                "«иначе»"
            } else {
                "«то»"
            };
            format!(
                "Условие (переменная #{var_id} {} {operand}) {verdict}: переменной #{var_id} \
                 присвоено значение {value} ранее в этом списке команд (Control Variables) — \
                 сравнение не может дать другой результат, поэтому ветка {dead} никогда не \
                 выполняется (мёртвый код). Достоверность likely: лёгкая constant-propagation в \
                 пределах списка команд.",
                op.symbol()
            )
        }
        Msg::PluginConflict {
            method,
            plugins,
            overwriters,
        } => {
            let who = plugins.join(", ");
            let over = overwriters.join(", ");
            format!(
                "Метод {method} патчат несколько включённых плагинов (в порядке загрузки): {who}. \
                 Перетирают исходную реализацию (без сохранения alias): {over} — поздний по порядку \
                 молча затирает логику более раннего, поведение зависит от порядка в plugins.js. \
                 Достоверность likely (AST-эвристика): проверьте совместимость и порядок загрузки."
            )
        }
    }
}

/// Renders the common-event cycle as `CE#a → CE#b → CE#a` (language-neutral).
fn render_cycle(cycle: &[u32]) -> String {
    cycle
        .iter()
        .map(|id| format!("CE#{id}"))
        .collect::<Vec<_>>()
        .join(" → ")
}

/// English message templates.
fn render_en(msg: &Msg) -> String {
    match msg {
        Msg::DeadVariable { id, name, writes } => {
            let label = name_label(name, Lang::En);
            format!(
                "Variable #{id}{label} is written ({writes}x) but never read — \
                 dead state: the write has no effect."
            )
        }
        Msg::UninitializedSymbol {
            kind,
            id,
            name,
            reads,
            plugin_checked,
        } => {
            let label = name_label(name, Lang::En);
            let kind = symbol_kind_label(*kind, Lang::En);
            let caveat = if *plugin_checked {
                " Cross-checked against plugin @param: the symbol is not declared by any enabled \
                 plugin."
            } else {
                " Not cross-checked against plugin @param (plugins.js not parsed): the value \
                 could be set by a plugin."
            };
            format!(
                "{kind} #{id}{label} is read ({reads}x) but never set — \
                 the condition may never fire / the value stays at its default.{caveat}"
            )
        }
        Msg::BrokenTransfer { map_id } => {
            format!(
                "Transfer Player to map #{map_id}, which does not exist in the project — \
                 the game will crash when the event fires."
            )
        }
        Msg::BrokenTransferVar { map_id } => {
            format!(
                "Variable Transfer Player to map #{map_id}, which does not exist in the project: \
                 the variable was assigned this value earlier in the same event \
                 (constant propagation). The game will crash when it fires with that value. \
                 Confidence likely: the value may have been overwritten outside static analysis."
            )
        }
        Msg::VehicleStartMapMissing { vehicle, map_id } => {
            format!(
                "The {} start map (System.json) points to map #{map_id}, which does not exist in \
                 the project — the vehicle will not appear and cannot be boarded (dead content). \
                 If intentional, the vehicle is repositioned via a Set Vehicle Location (202) \
                 event command.",
                vehicle_label(*vehicle, Lang::En)
            )
        }
        Msg::UnreachableMap { map_id, name } => {
            format!(
                "Map #{map_id} \"{name}\" is unreachable via direct transfers from the start map — \
                 it may be opened by a variable transfer, a plugin or a common event \
                 (not tracked yet)."
            )
        }
        Msg::DanglingDbRef { kind, id } => {
            format!(
                "Reference to {} #{id} ({}), which is not in the database — dangling reference.",
                db_kind_label(*kind, Lang::En),
                kind.file_stem()
            )
        }
        Msg::BrokenAsset { folder, name } => {
            format!(
                "Reference to asset \"{name}\" in {folder}/, which is not on disk — \
                 it will not load."
            )
        }
        Msg::OrphanAsset { folder, name } => {
            format!(
                "File \"{name}\" in {folder}/ is present but referenced nowhere in the data — \
                 possibly unused (plugin references are not considered yet)."
            )
        }
        Msg::DeadCodeAfterExit { code } => {
            format!(
                "Unreachable command (code {code}) after exiting the event — \
                 execution will never reach it."
            )
        }
        Msg::DeadSelfSwitch { ch, event } => {
            format!(
                "Self switch {ch} of event #{event} is set but never checked — \
                 dead self switch: the write has no effect."
            )
        }
        Msg::UnreachableSelfSwitch { ch, event } => {
            format!(
                "A page condition requires self switch {ch}, which no command of event \
                 #{event} ever sets — the page is unreachable. Self switches can be set by \
                 plugins/scripts ($gameSelfSwitches), which is not tracked yet."
            )
        }
        Msg::DeadCommonEvent { id, name } => {
            let label = name_label(&Some(name.clone()), Lang::En);
            format!(
                "Common event #{id}{label} has no trigger and no incoming caller (command 117 \
                 or effect 44) — it never runs. It may be reserved by a plugin/script \
                 ($gameTemp.reserveCommonEvent), which is not tracked yet."
            )
        }
        Msg::CyclicCommonEvents { cycle } => {
            let chain = render_cycle(cycle);
            format!(
                "Cycle in mutual common-event calls (command 117): {chain} — \
                 infinite synchronous recursion when triggered."
            )
        }
        Msg::ShadowedPage {
            page,
            by_page,
            event,
        } => {
            format!(
                "Page {page} of event #{event} is unreachable: the later page {by_page} \
                 (higher index, looser conditions) is always active whenever page {page} is, \
                 and wins over it — RPG Maker picks the highest-index page whose conditions are met."
            )
        }
        Msg::StuckAutorun { page, event } => {
            format!(
                "Autorun page {page} of event #{event} is gated by a condition but does nothing that \
                 could turn it off (no self switch / switch write, no player transfer, no common-event \
                 call / plugin command / script) — Autorun blocks input while active, so the game \
                 soft-locks (freezes). Accounted for and excluded: switches/variables declared by \
                 plugins (@type) or written by their JS ($gameSwitches.setValue); a gating switch \
                 turned off somewhere (121 OFF) or never turned on; pages with a script / plugin \
                 command / common-event call. Computed ids remain untracked — confidence likely."
            )
        }
        Msg::PluginLoadOrder {
            plugin,
            dependency,
            tag,
        } => {
            let req = match tag {
                PluginOrderTag::Base => format!(
                    "@base \"{dependency}\" (plugin \"{plugin}\" depends on it and must load later)"
                ),
                PluginOrderTag::OrderAfter => format!(
                    "@orderAfter \"{dependency}\" (plugin \"{plugin}\" must load after \"{dependency}\")"
                ),
                PluginOrderTag::OrderBefore => format!(
                    "@orderBefore \"{dependency}\" (plugin \"{plugin}\" must load before \"{dependency}\")"
                ),
            };
            format!(
                "Plugin load order violated: {req}, but plugins.js has them in the opposite order — \
                 the plugin may initialize before its dependency and crash or misbehave. \
                 Reorder them in the Plugin Manager."
            )
        }
        Msg::MissingBase {
            plugin,
            base,
            disabled,
        } => {
            let state = if *disabled {
                "is present but disabled"
            } else {
                "is missing from plugins.js"
            };
            format!(
                "Plugin \"{plugin}\" declares @base \"{base}\", but the base plugin {state} — \
                 \"{plugin}\" will not get the code it requires and will most likely crash on load. \
                 Add/enable \"{base}\"."
            )
        }
        Msg::UnknownPluginCommand {
            plugin,
            command,
            structured,
            plugin_loaded,
        } => {
            let who = match plugin {
                Some(p) => format!("plugin \"{p}\" command \"{command}\""),
                None => format!("command \"{command}\""),
            };
            if *plugin_loaded {
                format!(
                    "Plugin command call: {who} is not registered by that (enabled) plugin — \
                     neither via @command nor via PluginManager.registerCommand. Looks like a typo \
                     in the command name: the call will silently do nothing."
                )
            } else if *structured {
                format!(
                    "Plugin command call: {who} is not registered by any enabled plugin — \
                     a typo, a disabled plugin, or a missing plugin: the command will not run."
                )
            } else {
                format!(
                    "Plugin command call (MV): {who} is not found among the @command of \
                     enabled plugins — possibly a typo or a disabled plugin. MV does not separate \
                     the plugin name from the command, so this match is best-effort."
                )
            }
        }
        Msg::ImpossibleCondition {
            var_id,
            value,
            op,
            operand,
            result,
        } => {
            let verdict = if *result {
                "always true"
            } else {
                "always false"
            };
            let dead = if *result { "\"else\"" } else { "\"then\"" };
            format!(
                "Conditional branch (variable #{var_id} {} {operand}) is {verdict}: variable \
                 #{var_id} was set to {value} earlier in this command list (Control Variables), so \
                 the comparison cannot evaluate any other way — the {dead} branch never runs \
                 (dead code). Confidence likely: light constant propagation within the command list.",
                op.symbol()
            )
        }
        Msg::PluginConflict {
            method,
            plugins,
            overwriters,
        } => {
            let who = plugins.join(", ");
            let over = overwriters.join(", ");
            format!(
                "Method {method} is patched by several enabled plugins (in load order): {who}. \
                 These overwrite the original implementation (without keeping an alias): {over} — \
                 the later one silently clobbers the earlier one's logic, so behaviour depends on \
                 the order in plugins.js. Confidence likely (AST heuristic): check compatibility \
                 and load order."
            )
        }
    }
}

/// Renders a "chrome" service string in the chosen language.
///
/// Returns it **without** ANSI coloring: color is applied by the CLI renderer.
pub fn render_chrome(chrome: &Chrome, lang: Lang) -> String {
    match lang {
        Lang::Ru => render_chrome_ru(chrome),
        Lang::En => render_chrome_en(chrome),
    }
}

/// Russian "chrome" templates.
fn render_chrome_ru(chrome: &Chrome) -> String {
    match chrome {
        Chrome::HeadingError => "ОШИБКИ".to_string(),
        Chrome::HeadingWarning => "ПРЕДУПРЕЖДЕНИЯ".to_string(),
        Chrome::HeadingInfo => "ИНФОРМАЦИЯ".to_string(),
        Chrome::NoProblems => "Проблем не найдено.".to_string(),
        Chrome::Summary {
            errors,
            warnings,
            infos,
        } => format!("{errors} ошибок, {warnings} предупреждений, {infos} информационных"),
        Chrome::SummaryPrefix => "Итог:".to_string(),
        Chrome::HintPrefix => "Подсказка:".to_string(),
        Chrome::Related => "связано:".to_string(),
        Chrome::TagLikely => "(likely)".to_string(),
        Chrome::OrphansHint => "orphan-assets отключён по умолчанию (шумит на стоковом RTP). \
             Показать неиспользуемые ассеты: --orphans"
            .to_string(),
        Chrome::DeadCommonEventsHint => "dead-common-event отключён по умолчанию: литеральный \
             $gameTemp.reserveCommonEvent(N) отслеживается, но плагины часто резервируют общие \
             события динамически (вычисляемый id / struct-параметр), что статике не видно. \
             Показать неиспользуемые общие события: --dead-common-events"
            .to_string(),
        Chrome::SuppressedNote { count } => {
            format!("{count} находок(и) скрыто через .dk-doctor.toml ([[suppress]]).")
        }
        Chrome::BaselineWritten { count } => {
            format!("Baseline записан: {count} отпечаток(ов).")
        }
        Chrome::NewFindingsNote { count } => {
            format!("{count} новых находок(и) относительно baseline.")
        }
        Chrome::ParseWarningsHeader { count } => format!(
            "{count} файл(ов) проекта не удалось разобрать — они пропущены, отчёт может быть \
             неполным:"
        ),
        Chrome::Header { engine } => format!("— анализ проекта (движок: {engine})"),
        Chrome::LoadError { kind, detail } => match kind {
            LoadErrorKind::NotFound => format!(
                "Это не похоже на проект RPG Maker: в папке нет data/. Убедитесь, что выбран \
                 корень проекта (где лежит Game.exe или папка data). Путь: {detail}"
            ),
            LoadErrorKind::NotAnalyzable => format!(
                "Не удалось прочитать данные RPG Maker: проект, похоже, зашифрован или это не \
                 MV/MZ. Зашифрованные проекты анализировать нельзя. Путь: {detail}"
            ),
            LoadErrorKind::Io => format!("Ошибка чтения файлов проекта: {detail}"),
            LoadErrorKind::ParseError => format!("Не удалось разобрать файл проекта: {detail}"),
        },
    }
}

/// English "chrome" templates.
fn render_chrome_en(chrome: &Chrome) -> String {
    match chrome {
        Chrome::HeadingError => "ERRORS".to_string(),
        Chrome::HeadingWarning => "WARNINGS".to_string(),
        Chrome::HeadingInfo => "INFO".to_string(),
        Chrome::NoProblems => "No problems found.".to_string(),
        Chrome::Summary {
            errors,
            warnings,
            infos,
        } => format!("{errors} errors, {warnings} warnings, {infos} info"),
        Chrome::SummaryPrefix => "Total:".to_string(),
        Chrome::HintPrefix => "Hint:".to_string(),
        Chrome::Related => "related:".to_string(),
        Chrome::TagLikely => "(likely)".to_string(),
        Chrome::OrphansHint => "orphan-assets is off by default (noisy on stock RTP). \
             Show unused assets: --orphans"
            .to_string(),
        Chrome::DeadCommonEventsHint => {
            "dead-common-event is off by default: a literal $gameTemp.reserveCommonEvent(N) is \
             tracked, but plugins often reserve common events dynamically (computed id / struct \
             parameter), which static analysis cannot see. \
             Show unused common events: --dead-common-events"
                .to_string()
        }
        Chrome::SuppressedNote { count } => {
            format!("{count} finding(s) hidden via .dk-doctor.toml ([[suppress]]).")
        }
        Chrome::BaselineWritten { count } => {
            format!("Baseline written: {count} fingerprint(s).")
        }
        Chrome::NewFindingsNote { count } => {
            format!("{count} new finding(s) relative to the baseline.")
        }
        Chrome::ParseWarningsHeader { count } => format!(
            "{count} project file(s) could not be parsed — skipped, the report may be incomplete:"
        ),
        Chrome::Header { engine } => format!("— project analysis (engine: {engine})"),
        Chrome::LoadError { kind, detail } => match kind {
            LoadErrorKind::NotFound => format!(
                "This doesn't look like an RPG Maker project: there's no data/ folder. Make sure \
                 you picked the project root (the folder with Game.exe or a data folder). \
                 Path: {detail}"
            ),
            LoadErrorKind::NotAnalyzable => format!(
                "Couldn't read the RPG Maker data: the project looks encrypted, or it isn't \
                 MV/MZ. Encrypted projects can't be analyzed. Path: {detail}"
            ),
            LoadErrorKind::Io => format!("Error reading the project files: {detail}"),
            LoadErrorKind::ParseError => format!("Couldn't parse a project file: {detail}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_differ_between_languages() {
        let msg = Msg::OrphanAsset {
            folder: "img/pictures".to_string(),
            name: "Unused".to_string(),
        };
        let ru = render(&msg, Lang::Ru);
        let en = render(&msg, Lang::En);
        assert_ne!(ru, en);
        assert!(ru.contains("возможно"));
        assert!(en.contains("possibly unused"));
        // The asset name is language-neutral and present in both.
        assert!(ru.contains("Unused"));
        assert!(en.contains("Unused"));
    }

    #[test]
    fn dangling_ref_renders_db_label_per_language() {
        let msg = Msg::DanglingDbRef {
            kind: DbKind::Item,
            id: 99,
        };
        let ru = render(&msg, Lang::Ru);
        let en = render(&msg, Lang::En);
        assert_ne!(ru, en);
        assert!(ru.contains("предмет #99"));
        assert!(en.contains("item #99"));
        // The stable database file label is present in both languages.
        assert!(ru.contains("Items"));
        assert!(en.contains("Items"));
    }

    #[test]
    fn chrome_renders_per_language() {
        let ru = render_chrome(&Chrome::HeadingError, Lang::Ru);
        let en = render_chrome(&Chrome::HeadingError, Lang::En);
        assert_eq!(ru, "ОШИБКИ");
        assert_eq!(en, "ERRORS");
    }
}
