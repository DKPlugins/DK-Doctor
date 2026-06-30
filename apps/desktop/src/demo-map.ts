// Map-mode demo harness — renders the Atlas (map) view with MOCK findings over a
// neutral RTP-tiled map (geometry borrowed from stock RPG Maker MZ RTP art).
// Browser-only: tiles/sprites are fetched from /demo-tiles (no Tauri invoke).
// URL params: ?lang=en|ru &theme=light|dark &regions=1 &event=<id>
import type {
  AtlasEvent,
  EventGraphic,
  Finding,
  Lang,
  MapAtlas,
  MapRender,
  ProjectStats,
  Report,
} from "./api";
import { emptyFilters, buildAtlasIndex } from "./group";
import { appbarHTML, eventPanelHTML, reportHTML, type State } from "./render";
import { mountCanvas } from "./atlas";
import { composeMap, composeRegions } from "./tileset";

const MAP_ID = 12;
const PROJECT = "Aetheria";

// --- Curated map locations (coords reused from a valid RTP map layout) -------
type Ev = { id: number; name: string; x: number; y: number; g: EventGraphic };
const ch = (characterName: string, characterIndex: number, direction = 2, pattern = 1): EventGraphic => ({
  characterName,
  characterIndex,
  direction,
  pattern,
  tileId: 0,
});

const EVENTS: Ev[] = [
  { id: 1, name: "Harbor Gate", x: 5, y: 0, g: ch("People1", 0) },
  { id: 2, name: "Old Mill", x: 7, y: 3, g: ch("Nature", 0) },
  { id: 3, name: "Shrine", x: 8, y: 3, g: ch("People2", 2) },
  { id: 4, name: "Stone Bridge", x: 8, y: 5, g: ch("People3", 1) },
  { id: 5, name: "Market Square", x: 8, y: 6, g: ch("People1", 4) },
  { id: 6, name: "Watchtower", x: 9, y: 6, g: ch("Actor1", 0) },
  { id: 7, name: "Crossroads", x: 12, y: 7, g: ch("People4", 3) },
  { id: 8, name: "Cellar Door", x: 13, y: 7, g: ch("People2", 5) },
  { id: 9, name: "The Weary Inn", x: 13, y: 8, g: ch("People1", 6) },
  { id: 10, name: "Old Well", x: 6, y: 9, g: ch("Nature", 2) },
  { id: 11, name: "Forest Path", x: 7, y: 9, g: ch("Nature", 4) },
  { id: 12, name: "Lookout", x: 15, y: 7, g: ch("People3", 7) },
];

// --- Curated findings (anchored to map/event so the atlas lights up) ---------
type Sev = Finding["severity"];
type Cat = Finding["category"];
type Conf = Finding["confidence"];
interface F {
  ev: number | null; // event id, or null for a project-level finding
  rule: string;
  severity: Sev;
  category: Cat;
  confidence: Conf;
  file: string;
  sub: string; // path tail after Map012/EVxxx (or full path when ev=null)
  en: string;
  ru: string;
  refs?: { file: string; path: string }[];
}

const F: F[] = [
  {
    ev: 1, rule: "broken-transfer", severity: "error", category: "reference", confidence: "certain",
    file: "data/Map012.json", sub: "page1/cmd12",
    en: "Transfer command targets map #23, which does not exist — the player is sent to a missing map and the game crashes at the gate.",
    ru: "Команда перехода ведёт на карту №23, которой нет — игрок проваливается на несуществующую карту, переход роняет игру у ворот.",
  },
  {
    ev: 1, rule: "broken-assets", severity: "error", category: "asset", confidence: "certain",
    file: "data/Map012.json", sub: "page1",
    en: "Event graphic references img/characters/People_alt.png — the file is missing, the event renders as a blank tile.",
    ru: "Графика события ссылается на img/characters/People_alt.png — файла нет, событие отображается пустым тайлом.",
  },
  {
    ev: 1, rule: "dead-variables", severity: "info", category: "dead-code", confidence: "certain",
    file: "data/Map012.json", sub: "page2/cmd4",
    en: "Variable #87 'gateBonus' is written here but never read anywhere — the value has no effect.",
    ru: "Переменная №87 «gateBonus» записывается здесь, но нигде не читается — значение ни на что не влияет.",
  },
  {
    ev: 2, rule: "broken-assets", severity: "error", category: "asset", confidence: "certain",
    file: "data/Map012.json", sub: "page1",
    en: "Event graphic references img/characters/Mill_old.png — the file is missing, the event renders as a blank tile.",
    ru: "Графика события ссылается на img/characters/Mill_old.png — файла нет, событие отображается пустым тайлом.",
  },
  {
    ev: 2, rule: "referential-integrity", severity: "warning", category: "reference", confidence: "certain",
    file: "data/Map012.json", sub: "page1/cmd7",
    en: "Shop command lists item #142, which is not defined in Items — the shop opens with a blank row.",
    ru: "Команда магазина содержит предмет №142, которого нет в базе Items — магазин откроется с пустой строкой.",
    refs: [{ file: "data/Items.json", path: "Item142" }],
  },
  {
    ev: 3, rule: "stuck-autorun", severity: "warning", category: "data", confidence: "certain",
    file: "data/Map012.json", sub: "page1",
    en: "Autorun page never turns its trigger switch off — it runs every frame and the map can soft-lock.",
    ru: "Страница с автозапуском не выключает свой переключатель-триггер — крутится каждый кадр, карта может зависнуть.",
  },
  {
    ev: 4, rule: "impossible-condition", severity: "warning", category: "data", confidence: "likely",
    file: "data/Map012.json", sub: "page2/cmd3",
    en: "Page requires switch #44 ON and the same switch OFF — the condition can never hold and the page is dead.",
    ru: "Страница требует переключатель №44 включённым и одновременно выключенным — условие невыполнимо, страница мертва.",
  },
  {
    ev: 5, rule: "unreachable-self-switch", severity: "warning", category: "dead-code", confidence: "likely",
    file: "data/Map012.json", sub: "",
    en: "Self-switch D is turned on but no page on this event ever checks it — the branch it unlocks is unreachable.",
    ru: "Селф-свитч D включается, но ни одна страница этого события его не проверяет — ветка, которую он открывал, недостижима.",
  },
  {
    ev: 5, rule: "shadowed-page", severity: "warning", category: "data", confidence: "likely",
    file: "data/Map012.json", sub: "page1",
    en: "Page 1 has the same conditions as page 2 below it — page 2 can never trigger and is effectively dead.",
    ru: "У страницы 1 те же условия, что и у страницы 2 ниже — страница 2 никогда не сработает и фактически мертва.",
  },
  {
    ev: 6, rule: "broken-transfer", severity: "error", category: "reference", confidence: "certain",
    file: "data/Map012.json", sub: "page1/cmd9",
    en: "Transfer points to map #0 — an unset destination drops the player onto a black screen.",
    ru: "Переход указывает на карту №0 — несконфигурированная цель отправляет игрока на чёрный экран.",
  },
  {
    ev: 7, rule: "shadowed-page", severity: "warning", category: "data", confidence: "likely",
    file: "data/Map012.json", sub: "page2",
    en: "A lower page repeats the conditions of the page above it — it can never trigger.",
    ru: "Нижняя страница повторяет условия страницы выше — она никогда не сработает.",
  },
  {
    ev: 8, rule: "dead-code-after-exit", severity: "info", category: "dead-code", confidence: "certain",
    file: "data/Map012.json", sub: "page1/cmd19",
    en: "Commands follow an Exit Event Processing at the same indent — they can never execute.",
    ru: "Команды стоят после «Завершить обработку события» на том же уровне — они никогда не выполнятся.",
  },
  {
    ev: 9, rule: "uninit-symbols", severity: "info", category: "data", confidence: "likely",
    file: "data/Map012.json", sub: "page1/cmd2",
    en: "Switch #45 is read before anything ever turns it on — on a fresh save the branch always takes the OFF path.",
    ru: "Переключатель №45 читается раньше, чем где-либо включается — на новом сохранении ветка всегда идёт по OFF.",
  },
  {
    ev: 12, rule: "dead-variables", severity: "info", category: "dead-code", confidence: "certain",
    file: "data/Map012.json", sub: "page1/cmd6",
    en: "Variable #91 'lookoutSeen' is written but never read — the flag has no effect.",
    ru: "Переменная №91 «lookoutSeen» записывается, но нигде не читается — флаг ни на что не влияет.",
  },
  // project-level (no map) — keeps the overview/project counters honest
  {
    ev: null, rule: "plugin-load-order", severity: "warning", category: "plugin-order", confidence: "certain",
    file: "js/plugins.js", sub: "VisuMZ_2_BattleSystemATB",
    en: "VisuMZ_2_BattleSystemATB loads before its base VisuMZ_1_BattleCore — the dependent plugin may error on boot.",
    ru: "VisuMZ_2_BattleSystemATB загружается раньше своей базы VisuMZ_1_BattleCore — зависимый плагин может упасть при старте.",
    refs: [{ file: "js/plugins.js", path: "VisuMZ_1_BattleCore" }],
  },
];

const params = new URLSearchParams(location.search);
const lang: Lang = params.get("lang") === "ru" ? "ru" : "en";
const theme: "light" | "dark" = params.get("theme") === "dark" ? "dark" : "light";
const regionsOn = params.get("regions") === "1";
const eventParam = params.get("event");
const selEvent = eventParam !== null ? Number(eventParam) : 6;

function buildReport(): Report {
  const findings: Finding[] = F.map((f) => {
    const path =
      f.ev === null
        ? f.sub
        : `Map${String(MAP_ID).padStart(3, "0")}/EV${String(f.ev).padStart(3, "0")}${f.sub ? "/" + f.sub : ""}`;
    return {
      rule: f.rule,
      severity: f.severity,
      category: f.category,
      confidence: f.confidence,
      file: f.file,
      path,
      message_key: f.rule,
      args: { key: f.rule },
      message: lang === "ru" ? f.ru : f.en,
      references: f.refs ?? [],
    };
  });
  const summary = {
    errors: findings.filter((x) => x.severity === "error").length,
    warnings: findings.filter((x) => x.severity === "warning").length,
    infos: findings.filter((x) => x.severity === "info").length,
  };
  return { engine: "mz", lang, summary, findings };
}

const atlasEvents: AtlasEvent[] = EVENTS.map((e) => ({
  id: e.id,
  name: e.name,
  x: e.x,
  y: e.y,
  graphic: e.g,
}));

const STATS: ProjectStats = { engine: "mz", maps: 38, events: 412, commands: 9786, plugins: 17, assets: 1240 };

async function loadGeom(): Promise<MapRender & { events: { id: number; x: number; y: number }[] }> {
  const res = await fetch("/demo-tiles/map.json");
  return await res.json();
}

async function loadBitmaps(names: string[]): Promise<(ImageBitmap | null)[]> {
  const out: (ImageBitmap | null)[] = new Array(9).fill(null);
  await Promise.all(
    names.slice(0, 9).map(async (name, i) => {
      if (!name) return;
      try {
        const r = await fetch(`/demo-tiles/tilesets/${name}.png`);
        out[i] = await createImageBitmap(await r.blob());
      } catch {
        out[i] = null;
      }
    }),
  );
  return out;
}

// Minimal browser re-implementation of sprites.ts (which uses Tauri readProjectImage).
function extractCharacter(
  sheet: ImageBitmap,
  name: string,
  index: number,
  direction: number,
  pattern: number,
): HTMLCanvasElement | null {
  const big = name.includes("$");
  const cols = big ? 3 : 12;
  const rows = big ? 4 : 8;
  const cw = sheet.width / cols;
  const chh = sheet.height / rows;
  if (cw <= 0 || chh <= 0) return null;
  const blockCol = big ? 0 : (index % 4) * 3;
  const blockRow = big ? 0 : Math.floor(index / 4) * 4;
  const col = blockCol + Math.min(2, Math.max(0, pattern));
  const dir = direction >= 2 && direction <= 8 ? direction : 2;
  const row = blockRow + (Math.floor(dir / 2) - 1);
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.round(cw));
  canvas.height = Math.max(1, Math.round(chh));
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(sheet, col * cw, row * chh, cw, chh, 0, 0, canvas.width, canvas.height);
  return canvas;
}

async function loadSprites(events: Ev[]): Promise<Map<number, HTMLCanvasElement>> {
  const sprites = new Map<number, HTMLCanvasElement>();
  const names = new Set(events.map((e) => e.g.characterName));
  const sheets = new Map<string, ImageBitmap | null>();
  await Promise.all(
    [...names].map(async (n) => {
      try {
        const r = await fetch(`/demo-tiles/characters/${n}.png`);
        sheets.set(n, await createImageBitmap(await r.blob()));
      } catch {
        sheets.set(n, null);
      }
    }),
  );
  for (const e of events) {
    const sheet = sheets.get(e.g.characterName);
    if (!sheet) continue;
    const cv = extractCharacter(sheet, e.g.characterName, e.g.characterIndex, e.g.direction, e.g.pattern);
    if (cv) sprites.set(e.id, cv);
  }
  return sprites;
}

const report = buildReport();
const atlas: MapAtlas[] = [
  { mapId: MAP_ID, name: `${PROJECT} — Overworld`, parentId: 0, width: 17, height: 13, events: atlasEvents },
];

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
  project: { path: "C:/Games/Aetheria", name: PROJECT },
  report,
  stats: STATS,
  scannedAt: Date.now() - 134000,
  filters: emptyFilters(),
  groupBy: "severity",
  expanded: new Set(),
  ignored: new Set(),
  drawer: null,
  recent: [],
  newOnly: false,
  reportMode: "atlas",
  atlas,
  atlasSel: MAP_ID,
  atlasEvent: selEvent,
  atlasNewOnly: false,
  atlasRegions: regionsOn,
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

// Mount the live atlas canvas with real RTP tiles + sprites.
async function mount(): Promise<void> {
  const canvas = document.getElementById("atlasCanvas") as HTMLCanvasElement | null;
  if (!canvas) return;
  const geom = await loadGeom();
  const render: MapRender = {
    width: geom.width,
    height: geom.height,
    tilesetId: geom.tilesetId,
    tilesetNames: geom.tilesetNames,
    data: geom.data,
  };
  const idx = buildAtlasIndex(report, state.ignored);
  const eventFindings = idx.byMapEvent.get(MAP_ID) ?? new Map<number, number[]>();
  const ctrl = mountCanvas(canvas, {
    map: atlas[0],
    eventFindings,
    report,
    regionsOn,
    onSelect: (eventId) => {
      state.atlasEvent = eventId;
      const panel = document.getElementById("atlasEventPanel");
      if (panel) panel.innerHTML = eventPanelHTML(state, MAP_ID, eventId);
    },
  });
  const bitmaps = await loadBitmaps(geom.tilesetNames);
  const tiles = composeMap(render, bitmaps);
  ctrl.setTiles(tiles);
  ctrl.setRegions(composeRegions(render));
  ctrl.setSprites(await loadSprites(EVENTS));
  if (typeof selEvent === "number") ctrl.select(selEvent);
  // Signal readiness for the screenshotter.
  (window as unknown as { __atlasReady?: boolean }).__atlasReady = true;
}
void mount();
