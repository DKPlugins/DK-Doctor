// Demo harness — renders the report dashboard with MOCK data (no Tauri).
// Used only to produce marketing screenshots; not part of the shipped app.
// URL params: ?lang=en|ru &theme=light|dark &drawer=<index> &group=severity|category|map
import type { Finding, Lang, Report, ProjectStats } from "./api";
import { emptyFilters, type GroupBy } from "./group";
import { appbarHTML, drawerHTML, reportHTML, type State } from "./render";

type Loc = { en: string; ru: string };

interface Mock {
  rule: string;
  severity: Finding["severity"];
  category: Finding["category"];
  confidence: Finding["confidence"];
  file: string;
  path: string;
  msg: Loc;
  refs?: { file: string; path: string }[];
}

const MOCKS: Mock[] = [
  {
    rule: "broken-transfer",
    severity: "error",
    category: "reference",
    confidence: "certain",
    file: "data/Map008.json",
    path: "Map008/EV003/page1/cmd12",
    msg: {
      en: "Transfer command targets map #23, which does not exist — the player is sent to a missing map and the game crashes at the door.",
      ru: "Команда перехода ведёт на карту №23, которой нет — игрок проваливается на несуществующую карту, переход роняет игру.",
    },
  },
  // broken-assets cluster (4) — collapses into one row + "+3 more"
  {
    rule: "broken-assets",
    severity: "error",
    category: "asset",
    confidence: "certain",
    file: "data/Map004.json",
    path: "Map004/EV007/page1",
    msg: {
      en: "Event graphic references img/characters/People_alt.png — the file is missing, the event renders as a blank tile.",
      ru: "Графика события ссылается на img/characters/People_alt.png — файла нет, событие отображается пустым тайлом.",
    },
  },
  {
    rule: "broken-assets",
    severity: "error",
    category: "asset",
    confidence: "certain",
    file: "data/Map004.json",
    path: "Map004/EV011/page2",
    msg: {
      en: "Event graphic references img/characters/People_alt.png — the file is missing, the event renders as a blank tile.",
      ru: "Графика события ссылается на img/characters/People_alt.png — файла нет, событие отображается пустым тайлом.",
    },
  },
  {
    rule: "broken-assets",
    severity: "error",
    category: "asset",
    confidence: "certain",
    file: "data/Map009.json",
    path: "Map009/EV002/page1",
    msg: {
      en: "Event graphic references img/characters/People_alt.png — the file is missing, the event renders as a blank tile.",
      ru: "Графика события ссылается на img/characters/People_alt.png — файла нет, событие отображается пустым тайлом.",
    },
  },
  {
    rule: "broken-assets",
    severity: "error",
    category: "asset",
    confidence: "certain",
    file: "data/System.json",
    path: "title1",
    msg: {
      en: "Title screen references img/titles1/Castle_old.png — the file is missing, the boot screen falls back to black.",
      ru: "Титульный экран ссылается на img/titles1/Castle_old.png — файла нет, загрузочный экран остаётся чёрным.",
    },
  },
  // warnings
  {
    rule: "stuck-autorun",
    severity: "warning",
    category: "data",
    confidence: "certain",
    file: "data/Map012.json",
    path: "Map012/EV005/page1",
    msg: {
      en: "Autorun page never turns its trigger switch off — it runs every frame and the map can soft-lock.",
      ru: "Страница с автозапуском не выключает свой переключатель-триггер — крутится каждый кадр, карта может зависнуть.",
    },
  },
  {
    rule: "impossible-condition",
    severity: "warning",
    category: "data",
    confidence: "likely",
    file: "data/Map012.json",
    path: "Map012/EV008/page2/cmd3",
    msg: {
      en: "Page requires switch #44 ON and the same switch OFF — the condition can never hold and the page is dead.",
      ru: "Страница требует переключатель №44 включённым и одновременно выключенным — условие невыполнимо, страница мертва.",
    },
  },
  {
    rule: "plugin-load-order",
    severity: "warning",
    category: "plugin-order",
    confidence: "certain",
    file: "js/plugins.js",
    path: "VisuMZ_2_BattleSystemATB",
    msg: {
      en: "VisuMZ_2_BattleSystemATB loads before its base VisuMZ_1_BattleCore — the dependent plugin may error on boot.",
      ru: "VisuMZ_2_BattleSystemATB загружается раньше своей базы VisuMZ_1_BattleCore — зависимый плагин может упасть при старте.",
    },
    refs: [{ file: "js/plugins.js", path: "VisuMZ_1_BattleCore" }],
  },
  {
    rule: "unreachable-self-switch",
    severity: "warning",
    category: "dead-code",
    confidence: "likely",
    file: "data/Map007.json",
    path: "Map007/EV004",
    msg: {
      en: "Self-switch D is turned on but no page on this event ever checks it — the branch it unlocks is unreachable.",
      ru: "Селф-свитч D включается, но ни одна страница этого события его не проверяет — ветка, которую он открывал, недостижима.",
    },
  },
  {
    rule: "referential-integrity",
    severity: "warning",
    category: "reference",
    confidence: "certain",
    file: "data/Troops.json",
    path: "Troop018/member2",
    msg: {
      en: "Troop #18 places enemy #88, which is not defined in Enemies — the battle loads with an empty slot.",
      ru: "Группа врагов №18 содержит врага №88, которого нет в базе Enemies — бой загрузится с пустым местом.",
    },
    refs: [{ file: "data/Enemies.json", path: "Enemy088" }],
  },
  {
    rule: "shadowed-page",
    severity: "warning",
    category: "data",
    confidence: "likely",
    file: "data/Map003.json",
    path: "Map003/EV009/page1",
    msg: {
      en: "Page 1 has the same conditions as page 2 below it — page 2 can never trigger and is effectively dead.",
      ru: "У страницы 1 те же условия, что и у страницы 2 ниже — страница 2 никогда не сработает и фактически мертва.",
    },
  },
  // infos
  {
    rule: "dead-variables",
    severity: "info",
    category: "dead-code",
    confidence: "certain",
    file: "data/CommonEvents.json",
    path: "CommonEvent034/cmd6",
    msg: {
      en: "Variable #87 'goldBonus' is written but never read anywhere — the value has no effect.",
      ru: "Переменная №87 «goldBonus» записывается, но нигде не читается — значение ни на что не влияет.",
    },
  },
  {
    rule: "uninit-symbols",
    severity: "info",
    category: "data",
    confidence: "likely",
    file: "data/Map002.json",
    path: "Map002/EV001/page1/cmd2",
    msg: {
      en: "Switch #45 is read before anything ever turns it on — on a fresh save the branch always takes the OFF path.",
      ru: "Переключатель №45 читается раньше, чем где-либо включается — на новом сохранении ветка всегда идёт по OFF.",
    },
  },
  {
    rule: "dead-common-event",
    severity: "info",
    category: "dead-code",
    confidence: "likely",
    file: "data/CommonEvents.json",
    path: "CommonEvent052",
    msg: {
      en: "Common event #52 'IntroCutscene' is never called by any event, plugin or trigger — it is dead content.",
      ru: "Общее событие №52 «IntroCutscene» не вызывается ни одним событием, плагином или триггером — это мёртвый контент.",
    },
  },
  {
    rule: "dead-code-after-exit",
    severity: "info",
    category: "dead-code",
    confidence: "certain",
    file: "data/Map006.json",
    path: "Map006/EV012/page1/cmd19",
    msg: {
      en: "Commands follow an Exit Event Processing at the same indent — they can never execute.",
      ru: "Команды стоят после «Завершить обработку события» на том же уровне — они никогда не выполнятся.",
    },
  },
];

function buildReport(lang: Lang): Report {
  const findings: Finding[] = MOCKS.map((m) => ({
    rule: m.rule,
    severity: m.severity,
    category: m.category,
    confidence: m.confidence,
    file: m.file,
    path: m.path,
    message_key: m.rule,
    args: { key: m.rule },
    message: m.msg[lang],
    references: m.refs ?? [],
  }));
  const summary = {
    errors: findings.filter((f) => f.severity === "error").length,
    warnings: findings.filter((f) => f.severity === "warning").length,
    infos: findings.filter((f) => f.severity === "info").length,
  };
  return { engine: "mz", lang, summary, findings };
}

const STATS: ProjectStats = {
  engine: "mz",
  maps: 38,
  events: 412,
  commands: 9786,
  plugins: 17,
  assets: 1240,
};

const params = new URLSearchParams(location.search);
const lang: Lang = params.get("lang") === "ru" ? "ru" : "en";
const theme: "light" | "dark" = params.get("theme") === "dark" ? "dark" : "light";
const groupBy = (params.get("group") as GroupBy) || "severity";
const drawerParam = params.get("drawer");
const drawer = drawerParam !== null ? Number(drawerParam) : null;

const state: State = {
  view: "report",
  settings: {
    theme,
    lang,
    density: "comfortable",
    orphans: false,
    deadCommonEvents: true,
    checkUpdates: true,
  },
  lang,
  theme,
  project: { path: "C:/Games/Aetheria", name: "Aetheria" },
  report: buildReport(lang),
  stats: STATS,
  scannedAt: Date.now() - 134000,
  filters: emptyFilters(),
  groupBy,
  expanded: new Set(),
  ignored: new Set(),
  drawer,
  recent: [],
  newOnly: false,
  reportMode: "list",
  atlasSel: null,
  atlasEvent: null,
  atlasNewOnly: false,
  atlasRegions: false,
};

const $ = (id: string) => document.getElementById(id)!;
const root = document.documentElement;
root.setAttribute("data-theme", theme);
root.setAttribute("data-density", "comfortable");
root.setAttribute("lang", lang);

$("app").dataset.state = "report";
$("appbar").innerHTML = appbarHTML(state);
const view = $("view");
view.className = "view view--report";
view.innerHTML = reportHTML(state);

if (drawer !== null) {
  $("drawer").innerHTML = drawerHTML(state);
  $("drawer").classList.add("is-open");
  $("scrim").classList.add("is-on");
}
