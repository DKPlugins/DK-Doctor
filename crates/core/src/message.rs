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

use crate::ir::{CmpOp, DbKind, PictureOp, VehicleKind};

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
    /// A constant-resolvable condition (command 111): symbolic-range propagation
    /// (Control Variables set/add/sub/random) pins the variable to a value range
    /// that makes the comparison always yield the same result → one of the
    /// branches is unreachable (dead code).
    ImpossibleCondition {
        /// Id of the variable in the condition.
        var_id: u32,
        /// Lower bound of the variable's propagated value range.
        value_lo: i64,
        /// Upper bound of the variable's propagated value range (== `value_lo`
        /// when the value is an exact constant).
        value_hi: i64,
        /// Comparison operator.
        op: CmpOp,
        /// Lower bound of the right operand's value range.
        operand_lo: i64,
        /// Upper bound of the right operand's value range.
        operand_hi: i64,
        /// What the condition is guaranteed to evaluate to (`true` → the "else"
        /// branch is dead; `false` → the "then" branch is dead).
        result: bool,
    },
    /// A progression deadlock among global switches (`circular-gate`): a switch is
    /// turned ON only by events that are themselves gated behind switches which
    /// (transitively) require it, so none of them can ever run — the content
    /// behind the cycle is unreachable (soft-lock).
    CircularGate {
        /// Representative (lowest-id) switch of the deadlock cycle.
        switch_id: u32,
        /// Switch name from the database, if set.
        name: Option<String>,
        /// All switch ids forming the deadlock cycle (ascending, includes
        /// `switch_id`).
        cycle: Vec<u32>,
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
    /// A Transfer Player (201) whose fixed destination lands on a tile impassable
    /// from all four directions — the player cannot move off it (soft-lock).
    TransferToBlockedTile {
        /// Target map id.
        map_id: u32,
        /// Destination tile x.
        x: u32,
        /// Destination tile y.
        y: u32,
    },
    /// The player's start position (System.json) is on a tile impassable from all
    /// four directions — the game starts frozen.
    StartInWall {
        /// Start map id.
        map_id: u32,
        /// Start tile x.
        x: u32,
        /// Start tile y.
        y: u32,
    },
    /// A picture is operated on (Move/Rotate/Tint/Erase) before it is Shown on the
    /// same command sequence — the operation targets a picture that does not exist
    /// yet (no effect).
    PictureBeforeShow {
        /// Picture id (RPG Maker picture slot).
        picture_id: u32,
        /// The offending operation.
        op: PictureOp,
    },
    /// An Autorun page with an empty command list — Autorun blocks input while
    /// active, so an empty one freezes the game (soft-lock).
    EmptyAutorunPage {
        /// Page number (1-based).
        page: u32,
        /// Id of the event the page belongs to.
        event: u32,
    },
    /// A Parallel page with an empty command list — it runs every frame but does
    /// nothing (likely forgotten content).
    EmptyParallelPage {
        /// Page number (1-based).
        page: u32,
        /// Id of the event the page belongs to.
        event: u32,
    },
    /// A database record that nothing references anywhere in the data — likely
    /// unused / unreachable content.
    UnusedDbRecord {
        /// Kind of the DB record.
        kind: DbKind,
        /// Record id.
        id: u32,
        /// Record name, if set.
        name: Option<String>,
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
    /// Hint about `--circular-gates`.
    CircularGatesHint,
    /// Hint about `--tiles`.
    TilesHint,
    /// Hint about `--db-reachability`.
    DbReachabilityHint,
    /// Hint about `--pictures`.
    PicturesHint,
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

/// Picture-operation name in the chosen language (for [`Msg::PictureBeforeShow`]).
fn picture_op_label(op: PictureOp, lang: Lang) -> &'static str {
    match lang {
        Lang::Ru => match op {
            PictureOp::Move => "Переместить картинку",
            PictureOp::Rotate => "Повернуть картинку",
            PictureOp::Tint => "Тонировать картинку",
            PictureOp::Erase => "Удалить картинку",
        },
        Lang::En => match op {
            PictureOp::Move => "Move Picture",
            PictureOp::Rotate => "Rotate Picture",
            PictureOp::Tint => "Tint Picture",
            PictureOp::Erase => "Erase Picture",
        },
    }
}

/// Renders an inclusive integer range as `N` (exact) or `N..M` (language-neutral).
fn range_label(lo: i64, hi: i64) -> String {
    if lo == hi {
        lo.to_string()
    } else {
        format!("{lo}..{hi}")
    }
}

/// Renders a switch-id list as `#a, #b, #c` (language-neutral).
fn switch_list(ids: &[u32]) -> String {
    ids.iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ")
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
            value_lo,
            value_hi,
            op,
            operand_lo,
            operand_hi,
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
            let operand = range_label(*operand_lo, *operand_hi);
            let value_clause = if value_lo == value_hi {
                format!(
                    "переменной #{var_id} присвоено значение {value_lo} ранее в этом списке команд \
                     (Control Variables)"
                )
            } else {
                format!(
                    "переменная #{var_id} на этом участке может принимать только значения \
                     {value_lo}..{value_hi} (Control Variables: set/add/sub/random)"
                )
            };
            format!(
                "Условие (переменная #{var_id} {} {operand}) {verdict}: {value_clause} — \
                 сравнение не может дать другой результат, поэтому ветка {dead} никогда не \
                 выполняется (мёртвый код). Достоверность likely: символьный диапазон в пределах \
                 списка команд.",
                op.symbol()
            )
        }
        Msg::CircularGate {
            switch_id,
            name,
            cycle,
        } => {
            let label = name_label(name, Lang::Ru);
            let others: Vec<u32> = cycle.iter().copied().filter(|id| id != switch_id).collect();
            let others_clause = if others.is_empty() {
                String::new()
            } else {
                format!(" (в связке с переключателями {})", switch_list(&others))
            };
            format!(
                "Переключатель #{switch_id}{label}{others_clause} — тупик прогрессии: он включается \
                 (Control Switches ON) только событиями, которые сами открываются переключателями из \
                 этой же связки, поэтому ни одно из них не может сработать первым — переключатель \
                 никогда не станет ON, а завязанный на него контент недостижим (soft-lock). Учтены и \
                 исключены переключатели, управляемые плагином (@type / $gameSwitches.setValue) или \
                 скриптом. Достоверность likely: включение плагин-командой (356/357) статикой не \
                 отслеживается."
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
        Msg::TransferToBlockedTile { map_id, x, y } => {
            format!(
                "Переход (Transfer Player) на карту #{map_id} в клетку ({x}, {y}), непроходимую со \
                 всех четырёх сторон (по флагам тайлсета) — игрок не сможет с неё сойти (soft-lock). \
                 Достоверность likely: плагины проходимости (по регионам, пиксельное движение) и \
                 события со сквозным проходом статикой не учитываются."
            )
        }
        Msg::StartInWall { map_id, x, y } => {
            format!(
                "Стартовая позиция игрока (System.json) — карта #{map_id}, клетка ({x}, {y}), \
                 непроходимая со всех четырёх сторон (по флагам тайлсета): игра начнётся с \
                 застрявшим персонажем (soft-lock). Достоверность likely: плагины проходимости \
                 статикой не учитываются."
            )
        }
        Msg::PictureBeforeShow { picture_id, op } => {
            format!(
                "«{}» для картинки #{picture_id} стоит раньше её показа (Show Picture) в том же \
                 списке команд — команда выполняется над ещё не показанной картинкой и ни на что не \
                 влияет. Достоверность likely: картинку мог показать вызванный ранее скрипт/общее \
                 событие.",
                picture_op_label(*op, Lang::Ru)
            )
        }
        Msg::EmptyAutorunPage { page, event } => {
            format!(
                "Автозапускаемая (Autorun) страница {page} события #{event} без условий и с пустым \
                 списком команд — Autorun блокирует ввод, пока активен, поэтому пустой автозапуск \
                 намертво вешает игру (soft-lock). Либо добавьте команды/условие, либо смените \
                 триггер."
            )
        }
        Msg::EmptyParallelPage { page, event } => {
            format!(
                "Параллельная (Parallel) страница {page} события #{event} без условий и с пустым \
                 списком команд — выполняется каждый кадр, но ничего не делает: похоже на \
                 забытый/недоделанный контент."
            )
        }
        Msg::UnusedDbRecord { kind, id, name } => {
            let label = name_label(name, Lang::Ru);
            let channels = unused_channels_ru(*kind);
            format!(
                "{} #{id}{label} ({}) нигде не используется в данных ({channels}) — вероятно, \
                 неиспользуемый контент. Достоверность likely: ссылки из плагинов/заметок (notetag) \
                 статикой не отслеживаются.",
                db_kind_label_cap_ru(*kind),
                kind.file_stem()
            )
        }
    }
}

/// Reference channels checked for an unused DB record (RU) — what "used" means.
/// The `db-reachability` rule only emits Enemy/Skill/Weapon/Armor; the other kinds
/// get the generic clause (kept exhaustive per the catalog convention).
fn unused_channels_ru(kind: DbKind) -> &'static str {
    match kind {
        DbKind::Enemy => "нет ни в одной группе врагов и не вызывается через «Превращение врага»",
        DbKind::Skill => {
            "не изучается ни одним классом, не даётся чертами/эффектами, не используется врагами \
             или событиями"
        }
        DbKind::Weapon | DbKind::Armor => {
            "не продаётся, не выпадает, не надевается и не выдаётся событиями"
        }
        DbKind::Actor
        | DbKind::Class
        | DbKind::Item
        | DbKind::Troop
        | DbKind::State
        | DbKind::Animation
        | DbKind::Tileset
        | DbKind::CommonEvent => "нет ссылок в данных",
    }
}

/// Capitalized DB-kind label (RU) for the start of an [`Msg::UnusedDbRecord`] line.
fn db_kind_label_cap_ru(kind: DbKind) -> &'static str {
    match kind {
        DbKind::Enemy => "Враг",
        DbKind::Skill => "Навык",
        DbKind::Weapon => "Оружие",
        DbKind::Armor => "Броня",
        DbKind::Actor => "Актёр",
        DbKind::Class => "Класс",
        DbKind::Item => "Предмет",
        DbKind::Troop => "Группа врагов",
        DbKind::State => "Состояние",
        DbKind::Animation => "Анимация",
        DbKind::Tileset => "Тайлсет",
        DbKind::CommonEvent => "Общее событие",
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
            value_lo,
            value_hi,
            op,
            operand_lo,
            operand_hi,
            result,
        } => {
            let verdict = if *result {
                "always true"
            } else {
                "always false"
            };
            let dead = if *result { "\"else\"" } else { "\"then\"" };
            let operand = range_label(*operand_lo, *operand_hi);
            let value_clause = if value_lo == value_hi {
                format!(
                    "variable #{var_id} was set to {value_lo} earlier in this command list \
                     (Control Variables)"
                )
            } else {
                format!(
                    "variable #{var_id} can only hold values {value_lo}..{value_hi} at this point \
                     (Control Variables: set/add/sub/random)"
                )
            };
            format!(
                "Conditional branch (variable #{var_id} {} {operand}) is {verdict}: {value_clause}, \
                 so the comparison cannot evaluate any other way — the {dead} branch never runs \
                 (dead code). Confidence likely: symbolic range within the command list.",
                op.symbol()
            )
        }
        Msg::CircularGate {
            switch_id,
            name,
            cycle,
        } => {
            let label = name_label(name, Lang::En);
            let others: Vec<u32> = cycle.iter().copied().filter(|id| id != switch_id).collect();
            let others_clause = if others.is_empty() {
                String::new()
            } else {
                format!(" (together with switches {})", switch_list(&others))
            };
            format!(
                "Switch #{switch_id}{label}{others_clause} — a progression deadlock: it is turned ON \
                 (Control Switches) only by events that are themselves gated by switches from the \
                 same cluster, so none of them can ever run first — the switch never becomes ON and \
                 the content behind it is unreachable (soft-lock). Switches managed by a plugin \
                 (@type / $gameSwitches.setValue) or a script are accounted for and excluded. \
                 Confidence likely: a plugin command (356/357) turning it on is not tracked \
                 statically."
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
        Msg::TransferToBlockedTile { map_id, x, y } => {
            format!(
                "Transfer Player to map #{map_id} at tile ({x}, {y}), which is impassable from all \
                 four directions (per the tileset flags) — the player cannot move off it \
                 (soft-lock). Confidence likely: passability plugins (region passage, pixel \
                 movement) and through-events are not accounted for."
            )
        }
        Msg::StartInWall { map_id, x, y } => {
            format!(
                "The player's start position (System.json) is map #{map_id}, tile ({x}, {y}), which \
                 is impassable from all four directions (per the tileset flags): the game starts \
                 with the character stuck (soft-lock). Confidence likely: passability plugins are \
                 not accounted for."
            )
        }
        Msg::PictureBeforeShow { picture_id, op } => {
            format!(
                "\"{}\" for picture #{picture_id} comes before it is shown (Show Picture) in the \
                 same command list — the command runs on a picture that does not exist yet and has \
                 no effect. Confidence likely: the picture may have been shown by an earlier \
                 script/common event.",
                picture_op_label(*op, Lang::En)
            )
        }
        Msg::EmptyAutorunPage { page, event } => {
            format!(
                "Autorun page {page} of event #{event} has no conditions and an empty command \
                 list — Autorun blocks input while active, so an empty autorun freezes the game \
                 (soft-lock). Add commands/a condition, or change the trigger."
            )
        }
        Msg::EmptyParallelPage { page, event } => {
            format!(
                "Parallel page {page} of event #{event} has no conditions and an empty command \
                 list — it runs every frame but does nothing: likely forgotten / unfinished \
                 content."
            )
        }
        Msg::UnusedDbRecord { kind, id, name } => {
            let label = name_label(name, Lang::En);
            let channels = unused_channels_en(*kind);
            format!(
                "{} #{id}{label} ({}) is referenced nowhere in the data ({channels}) — likely \
                 unused content. Confidence likely: references from plugins/notetags are not \
                 tracked statically.",
                db_kind_label_cap_en(*kind),
                kind.file_stem()
            )
        }
    }
}

/// Reference channels checked for an unused DB record (EN) — what "used" means.
/// The `db-reachability` rule only emits Enemy/Skill/Weapon/Armor; the other kinds
/// get the generic clause (kept exhaustive per the catalog convention).
fn unused_channels_en(kind: DbKind) -> &'static str {
    match kind {
        DbKind::Enemy => "not in any troop and not summoned via Enemy Transform",
        DbKind::Skill => {
            "not learned by any class, not granted by traits/effects, not used by enemies or events"
        }
        DbKind::Weapon | DbKind::Armor => "not sold, dropped, equipped, or granted by events",
        DbKind::Actor
        | DbKind::Class
        | DbKind::Item
        | DbKind::Troop
        | DbKind::State
        | DbKind::Animation
        | DbKind::Tileset
        | DbKind::CommonEvent => "no references in the data",
    }
}

/// Capitalized DB-kind label (EN) for the start of an [`Msg::UnusedDbRecord`] line.
fn db_kind_label_cap_en(kind: DbKind) -> &'static str {
    match kind {
        DbKind::Enemy => "Enemy",
        DbKind::Skill => "Skill",
        DbKind::Weapon => "Weapon",
        DbKind::Armor => "Armor",
        DbKind::Actor => "Actor",
        DbKind::Class => "Class",
        DbKind::Item => "Item",
        DbKind::Troop => "Troop",
        DbKind::State => "State",
        DbKind::Animation => "Animation",
        DbKind::Tileset => "Tileset",
        DbKind::CommonEvent => "Common event",
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
        Chrome::CircularGatesHint => "circular-gate отключён по умолчанию (прототип): ищет тупики \
             прогрессии — связки переключателей, которые взаимно блокируют включение друг друга. \
             Включение плагин-командами не отслеживается. Показать: --circular-gates"
            .to_string(),
        Chrome::TilesHint => "blocked-tile отключён по умолчанию: проверяет проходимость целевых \
             клеток (переход/старт игрока непроходим со всех сторон). Плагины проходимости \
             (регионы, пиксельное движение) не учитываются. Показать: --tiles"
            .to_string(),
        Chrome::DbReachabilityHint => "db-reachability отключён по умолчанию: ищет записи БД \
             (враги/навыки/оружие/броня), на которые нет ссылок в данных. Ссылки из \
             плагинов/заметок не отслеживаются. Показать: --db-reachability"
            .to_string(),
        Chrome::PicturesHint => "picture-lifecycle отключён по умолчанию: ищет операции с \
             картинкой (перемещение/поворот/тон/удаление) до её показа в том же списке команд. \
             Картинки живут между событиями, поэтому показ из другого события/скрипта статикой \
             не виден. Показать: --pictures"
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
        Chrome::CircularGatesHint => "circular-gate is off by default (prototype): it looks for \
             progression deadlocks — clusters of switches that mutually block each other from ever \
             turning on. A plugin command turning a switch on is not tracked. Show: --circular-gates"
            .to_string(),
        Chrome::TilesHint => "blocked-tile is off by default: it checks tile passability of fixed \
             destinations (a transfer / the player start landing on a tile blocked on all sides). \
             Passability plugins (regions, pixel movement) are not accounted for. Show: --tiles"
            .to_string(),
        Chrome::DbReachabilityHint => "db-reachability is off by default: it finds database records \
             (enemies/skills/weapons/armors) referenced nowhere in the data. Plugin/notetag \
             references are not tracked. Show: --db-reachability"
            .to_string(),
        Chrome::PicturesHint => "picture-lifecycle is off by default: it flags a picture operated \
             on (move/rotate/tint/erase) before it is shown in the same command list. Pictures \
             persist across events, so a show from another event/script is invisible to static \
             analysis. Show: --pictures"
            .to_string(),
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
