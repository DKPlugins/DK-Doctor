import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  type CommandLine,
  type Lang,
  type ScanResult,
  type UpdateInfo,
  checkUpdate,
  eventCommands,
  exportReport,
  mapAtlas,
  mapGraph,
  mapRender,
  onProjectChanged,
  openRelease,
  pickFolder,
  saveImagePng,
  saveTextFile,
  scan,
  systemLang,
  unwatchProject,
  watchProject,
} from "./api";
import type { MapRender } from "./api";
import { buildReportHtml } from "./exportHtml";
import { renderSummaryCard } from "./summaryCard";
import { composeMap, composeRegions, disposeTileset, loadTileset } from "./tileset";
import { buildEventSprites } from "./sprites";
import { computeHealth } from "./health";
import {
  buildAtlasIndex,
  diffReport,
  emptyFilters,
  fingerprintsOf,
  type GroupBy,
  newEventsOnMap,
  parseCommandLoc,
  visible,
} from "./group";
import { type AtlasController, mountCanvas } from "./atlas";
import { icon } from "./icons";
import { t } from "./i18n";
import {
  appbarHTML,
  commandContextHTML,
  drawerHTML,
  errorHTML,
  eventPanelHTML,
  mainHTML,
  newRestrict,
  overlayHTML,
  reportHTML,
  resolveAtlasSel,
  scanShellHTML,
  settingsHTML,
  type State,
  updateBarHTML,
  welcomeHTML,
} from "./render";
import {
  type AtlasViewport,
  type Settings,
  addRecent,
  appendHistory,
  clearRecent,
  clearSnapshots,
  loadAtlasMemory,
  loadHistory,
  loadRecent,
  loadSettings,
  loadSnapshot,
  removeHistory,
  removeRecent,
  removeSnapshot,
  saveAtlasMemory,
  saveSettings,
  saveSnapshot,
} from "./store";

// --- DOM nodes (stable skeleton from index.html) --------------------------
const $ = (id: string) => document.getElementById(id)!;
const appEl = $("app");
const appbarEl = $("appbar");
const updateBarEl = $("updateBar");
const viewEl = $("view");
const scrimEl = $("scrim");
const drawerEl = $("drawer");
const toastEl = $("toast");
const settingsEl = $("settings");
const settingsScrimEl = $("settingsScrim");
const overlayEl = $("overlay");
const overlayScrimEl = $("overlayScrim");

const mql = window.matchMedia("(prefers-color-scheme: dark)");
const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// --- State -----------------------------------------------------------------
const settings = loadSettings();
const state: State = {
  view: "welcome",
  settings,
  lang: "en",
  theme: "light",
  filters: emptyFilters(),
  groupBy: "severity",
  expanded: new Set(),
  ignored: new Set(),
  drawer: null,
  recent: loadRecent(),
  newOnly: false,
  reportMode: "atlas",
  atlasSel: null,
  atlasEvent: null,
  atlasNewOnly: false,
  atlasRegions: false,
  overlay: null,
};

/** Visible order of findings (for prev/next navigation in the drawer). */
let currentOrder: number[] = [];
/** Scan in progress — block repeated launches. */
let busy = false;
let toastTimer: number | undefined;
/** Live canvas controller for the atlas (destroyed on every re-render). */
let atlasController: AtlasController | null = null;
/** One-shot: event to recenter on after the next mount (the "show on map" jump). */
let pendingFocus: number | null = null;
/** Monotonic token invalidating stale async tile loads when the map changes. */
let tilesToken = 0;
/** Guards a single in-flight map-graph fetch per scan. */
let graphLoading = false;
/** Unsubscribe for the Watch Mode `project-changed` listener (null when off). */
let watchUnlisten: (() => void) | null = null;
/** Debounce timer coalescing rapid watch-triggered re-scans. */
let watchTimer: number | undefined;
/** Monotonic token invalidating a startWatch that lost a race with stopWatch. */
let watchToken = 0;

// --- Theme/language resolution ---------------------------------------------
const resolveTheme = (s: Settings): "light" | "dark" =>
  s.theme === "system" ? (mql.matches ? "dark" : "light") : s.theme;
const resolveLang = (s: Settings): Lang =>
  s.lang === "system" ? systemLang() : s.lang;

function applyChrome(): void {
  state.theme = resolveTheme(state.settings);
  state.lang = resolveLang(state.settings);
  const root = document.documentElement;
  root.setAttribute("data-theme", state.theme);
  root.setAttribute("data-density", state.settings.density);
  root.setAttribute("lang", state.lang);
}

// --- Utilities -------------------------------------------------------------
const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));
function baseName(p: string): string {
  const parts = p.split(/[\\/]/).filter(Boolean);
  return parts.length ? parts[parts.length - 1] : p;
}
function setView(cls: string, html: string): void {
  // A full view swap discards the old DOM; tear down the canvas controller so
  // its listeners/observer/probe don't leak (the atlas remounts after render).
  destroyAtlasController();
  viewEl.className = cls;
  viewEl.innerHTML = html;
}
function errMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}

// --- Render ----------------------------------------------------------------
function updateOrder(): void {
  currentOrder = state.report
    ? visible(state.report, state.filters, state.ignored, newRestrict(state)).map((x) => x.i)
    : [];
}

function renderReportView(): void {
  if (state.reportMode === "atlas" && state.atlasSel === null) {
    state.atlasSel = resolveAtlasSel(state);
  }
  const prev = document.getElementById("reportMain");
  const scroll = prev ? prev.scrollTop : 0;
  setView("view view--report", reportHTML(state));
  const main = document.getElementById("reportMain");
  if (main) main.scrollTop = scroll;
  updateOrder();
  if (state.reportMode === "atlas") mountAtlasView();
  else if (state.reportMode === "graph") void ensureGraph();
}

/**
 * Lazily fetches the map-transition graph the first time the graph view is
 * shown. Failure is non-fatal — an empty graph renders the "no transfers" state
 * instead of blocking the view.
 */
async function ensureGraph(): Promise<void> {
  if (state.graph || graphLoading || !state.project) return;
  graphLoading = true;
  const path = state.project.path;
  try {
    const g = await mapGraph(path);
    if (state.project?.path === path) state.graph = g;
  } catch {
    if (state.project?.path === path) {
      state.graph = { startMapId: 0, nodes: [], edges: [] };
    }
  } finally {
    graphLoading = false;
  }
  if (state.view === "report" && state.reportMode === "graph") renderReportView();
}

/** Tears down the live atlas canvas controller (idempotent). */
function destroyAtlasController(): void {
  tilesToken++; // abort any in-flight tile load targeting the old controller
  if (atlasController) {
    atlasController.destroy();
    atlasController = null;
  }
}

/** Mounts the canvas controller for the currently-selected atlas map. */
function mountAtlasView(): void {
  destroyAtlasController();
  const canvas = document.getElementById("atlasCanvas") as HTMLCanvasElement | null;
  if (!canvas || !state.report || !state.atlas) return;
  const sel = state.atlasSel;
  if (typeof sel !== "number") return;
  const map = state.atlas.find((m) => m.mapId === sel);
  if (!map) return;
  const idx = buildAtlasIndex(state.report, state.ignored);
  const eventFindings = idx.byMapEvent.get(sel) ?? new Map<number, number[]>();
  const newEvents =
    state.diff && !state.diff.baseline ? newEventsOnMap(idx, sel, state.diff) : null;
  const path = state.project?.path;
  const saved = path ? loadAtlasMemory(path) : null;
  const initView =
    saved?.viewport && saved.viewport.mapId === sel
      ? { ox: saved.viewport.ox, oy: saved.viewport.oy, cell: saved.viewport.cell }
      : null;
  atlasController = mountCanvas(canvas, {
    map,
    eventFindings,
    report: state.report,
    newEvents,
    initView,
    regionsOn: state.atlasRegions,
    onSelect: (eventId) => {
      state.atlasEvent = eventId;
      const panel = document.getElementById("atlasEventPanel");
      if (panel) panel.innerHTML = eventPanelHTML(state, sel, eventId);
    },
    // Capture this controller's map id (`sel`), not the live state.atlasSel:
    // a flush on teardown can fire after the selection has already moved on.
    onView: (v) => persistAtlas({ mapId: sel, ox: v.ox, oy: v.oy, cell: v.cell }),
  });
  const wire = (id: string, fn: () => void) => {
    document.getElementById(id)?.addEventListener("click", fn);
  };
  wire("atlasZoomIn", () => atlasController?.zoom(1.25));
  wire("atlasZoomOut", () => atlasController?.zoom(1 / 1.25));
  wire("atlasFit", () => atlasController?.fit());
  wire("atlasWorst", () => atlasController?.goToWorst());
  wire("atlasExport", () => void doExportPng());
  wire("atlasRegions", () => {
    const on = atlasController?.toggleRegions() ?? false;
    state.atlasRegions = on;
    persistAtlas();
    document.getElementById("atlasRegions")?.classList.toggle("is-on", on);
  });
  // The "show on map" jump recenters once; an ordinary remount (tree toggle,
  // "only new", etc.) only restores the selection highlight without recentering,
  // so the user's pan/zoom (and the restored viewport) is preserved.
  if (pendingFocus !== null) {
    atlasController.focus(pendingFocus);
    pendingFocus = null;
  } else if (typeof state.atlasEvent === "number") {
    atlasController.select(state.atlasEvent);
  }
  void loadMapTiles(sel);
}

/** Persists the current atlas selection / viewport / region toggle per project. */
function persistAtlas(vp?: AtlasViewport): void {
  const path = state.project?.path;
  if (!path) return;
  const prev = loadAtlasMemory(path);
  const sel = state.atlasSel;
  const memSel: number | "project" | "overview" =
    typeof sel === "number" || sel === "project" || sel === "overview" ? sel : "overview";
  saveAtlasMemory(path, {
    sel: memSel,
    viewport: vp ?? prev?.viewport ?? null,
    regions: state.atlasRegions,
  });
}

/** Exports the current atlas map as an annotated PNG via the save dialog. */
async function doExportPng(): Promise<void> {
  if (!atlasController) return;
  const url = atlasController.toDataURL();
  const base = state.project?.name ?? "map";
  const name =
    typeof state.atlasSel === "number" ? `${base}-map${state.atlasSel}.png` : `${base}.png`;
  try {
    const ok = await saveImagePng(url, name, t(state.lang, "dlgSavePng"));
    if (ok) toast(t(state.lang, "pngSaved"));
  } catch {
    toast(t(state.lang, "pngFailed"));
  }
}

/** Loads real tileset art for the given map and applies it under the events. */
async function loadMapTiles(mapId: number): Promise<void> {
  const token = ++tilesToken;
  const ctrl = atlasController;
  const path = state.project?.path;
  if (!ctrl || !path) return;
  let bitmaps: (ImageBitmap | null)[] | null = null;
  let render: MapRender | null = null;
  let tilesOk = false;
  try {
    render = await mapRender(path, mapId);
    if (token !== tilesToken) return;
    bitmaps = await loadTileset(path, render.tilesetNames);
    if (token !== tilesToken) return;
    const tiles = composeMap(render, bitmaps);
    if (token !== tilesToken) return;
    // Apply tiles FIRST — they must not depend on the (separate, fallible)
    // sprite build below. The region overlay shares the same render data.
    ctrl.setTiles(tiles);
    ctrl.setRegions(composeRegions(render));
    tilesOk = !!tiles;
  } catch (e) {
    // Tiles are optional (the schematic stays) — but log so a blank map can be
    // diagnosed from the devtools console.
    console.warn("atlas: tile render failed", e);
  }

  // Non-blocking hint when a map declares tiles we couldn't render (encrypted
  // without key / nonstandard format) — the schematic remains usable.
  if (token === tilesToken) {
    const wanted = !!render && render.tilesetNames.some(Boolean);
    setTilesHint(wanted && !tilesOk);
  }

  // Event sprites are best-effort and decoupled: a failure here must never wipe
  // the tiles already applied above.
  try {
    const atlasMap = state.atlas?.find((m) => m.mapId === mapId) ?? null;
    if (atlasMap && bitmaps) {
      const sprites = await buildEventSprites(path, atlasMap, bitmaps);
      if (token === tilesToken) ctrl.setSprites(sprites);
    }
  } catch (e) {
    console.warn("atlas: event sprites failed", e);
  } finally {
    disposeTileset(bitmaps);
  }
}

/** Shows/hides the "tiles unavailable" hint over the atlas canvas. */
function setTilesHint(show: boolean): void {
  const wrap = document.querySelector(".atlas__canvaswrap");
  if (!wrap) return;
  let hint = wrap.querySelector(".atlas__tilesna") as HTMLElement | null;
  if (show) {
    if (!hint) {
      hint = document.createElement("div");
      hint.className = "atlas__tilesna";
      wrap.appendChild(hint);
    }
    hint.innerHTML = `${icon("triangle-alert")}<span></span>`;
    const span = hint.querySelector("span");
    if (span) span.textContent = t(state.lang, "atlasTilesNa");
  } else if (hint) {
    hint.remove();
  }
}

/** Switches the report sub-view (atlas/graph/list). */
function setReportMode(mode: "atlas" | "list" | "graph"): void {
  if (state.reportMode === mode) return;
  state.reportMode = mode;
  renderReportView();
  syncDrawer();
}

// --- Overlay modals (readiness / timeline / export) ------------------------
/** Shows/hides the overlay modal based on `state.overlay`. */
function syncOverlay(): void {
  const open = state.overlay !== null && state.view === "report";
  if (open) {
    overlayEl.innerHTML = overlayHTML(state);
    overlayEl.classList.add("is-open");
    overlayScrimEl.classList.add("is-on");
  } else {
    overlayEl.classList.remove("is-open");
    overlayScrimEl.classList.remove("is-on");
  }
}
function openOverlay(kind: "readiness" | "timeline" | "export"): void {
  if (!state.report) return;
  if (kind === "timeline" && state.project) {
    state.history = loadHistory(state.project.path);
  }
  state.overlay = kind;
  syncOverlay();
}
function closeOverlay(): void {
  state.overlay = null;
  syncOverlay();
}

/** Selects an atlas target (a map id, the project board, or the overview). */
function selectAtlas(sel: number | "project" | "overview"): void {
  state.atlasSel = sel;
  state.atlasEvent = null;
  persistAtlas();
  renderReportView();
}

/** Repaint only the main area (e.g. pattern expand/collapse), preserving scroll. */
function repaintMain(): void {
  const main = document.getElementById("reportMain");
  if (!main) return;
  const scroll = main.scrollTop;
  main.innerHTML = mainHTML(state);
  main.scrollTop = scroll;
  updateOrder();
  syncActiveRow();
}

function paint(): void {
  applyChrome();
  appEl.dataset.state = state.view;
  appbarEl.innerHTML = appbarHTML(state);
  if (state.view === "welcome") setView("view view--welcome", welcomeHTML(state));
  else if (state.view === "error") setView("view view--center", errorHTML(state));
  else if (state.view === "report") renderReportView();
  // scanning draws #view itself (see startScan)
  syncDrawer();
  syncOverlay();
}

// --- Finding drawer --------------------------------------------------------
function syncActiveRow(): void {
  const cur = state.drawer === null ? "" : String(state.drawer);
  document.querySelectorAll<HTMLElement>(".frow").forEach((r) => {
    r.classList.toggle("is-active", r.dataset.find === cur);
  });
}
/** Cache of fetched event-page command lists, keyed by `map/event/page`. */
const cmdContextCache = new Map<string, CommandLine[]>();

function syncDrawer(): void {
  const open = state.drawer !== null && state.view === "report";
  if (open) {
    drawerEl.innerHTML = drawerHTML(state);
    drawerEl.classList.add("is-open");
    scrimEl.classList.add("is-on");
    void loadDrawerContext();
  } else {
    drawerEl.classList.remove("is-open");
    scrimEl.classList.remove("is-on");
  }
  syncActiveRow();
}

/**
 * Fills the drawer's context block with the offending event page's command
 * list, highlighting the line the finding points at. No-op for findings that
 * are not anchored to a specific command.
 */
async function loadDrawerContext(): Promise<void> {
  if (state.drawer === null || !state.report || !state.project) return;
  const at = state.drawer;
  const f = state.report.findings[at];
  if (!f) return;
  const loc = parseCommandLoc(f);
  if (!loc) return;
  const key = `${loc.map}/${loc.event}/${loc.page}`;
  let lines = cmdContextCache.get(key);
  if (!lines) {
    try {
      lines = await eventCommands(state.project.path, loc.map, loc.event, loc.page);
    } catch {
      return;
    }
    cmdContextCache.set(key, lines);
  }
  if (state.drawer !== at) return; // drawer moved while awaiting
  const host = document.getElementById("drawerContext");
  if (!host) return;
  host.innerHTML = commandContextHTML(state.lang, lines, loc.cmd);
  host.querySelector<HTMLElement>('[data-hit="1"]')?.scrollIntoView({ block: "nearest" });
}
function openDrawer(i: number): void {
  if (!state.report || !state.report.findings[i]) return;
  state.drawer = i;
  syncDrawer();
}
function closeDrawer(): void {
  state.drawer = null;
  syncDrawer();
}
function navDrawer(dir: number): void {
  if (!currentOrder.length) return;
  let i = state.drawer === null ? -1 : currentOrder.indexOf(state.drawer);
  i = i < 0 ? 0 : (i + dir + currentOrder.length) % currentOrder.length;
  openDrawer(currentOrder[i]);
}

// --- Settings --------------------------------------------------------------
function openSettings(): void {
  settingsEl.innerHTML = settingsHTML(state);
  settingsScrimEl.classList.add("is-on");
  settingsEl.classList.add("is-open");
}
function closeSettings(): void {
  settingsScrimEl.classList.remove("is-on");
  settingsEl.classList.remove("is-open");
}
function applySettings(partial: Partial<Settings>): void {
  const beforeLang = state.lang;
  state.settings = { ...state.settings, ...partial };
  saveSettings(state.settings);
  applyChrome();
  appbarEl.innerHTML = appbarHTML(state);

  const langChanged = state.lang !== beforeLang;
  if (langChanged && state.view === "report" && state.project) {
    // Finding messages are localized by the engine → a re-analysis is needed.
    closeSettings();
    void startScan(state.project.path);
    return;
  }
  if (langChanged) {
    if (state.view === "welcome") setView("view view--welcome", welcomeHTML(state));
    else if (state.view === "error") setView("view view--center", errorHTML(state));
  }
  if (settingsEl.classList.contains("is-open")) settingsEl.innerHTML = settingsHTML(state);
}

// --- Toast -----------------------------------------------------------------
function toast(text: string): void {
  // `text` may be an arbitrary error string (OS/dialog/backend) — insert it as
  // textContent, never as HTML, so markup characters can't break or inject DOM.
  toastEl.innerHTML = `${icon("terminal")}<span></span>`;
  const span = toastEl.querySelector("span");
  if (span) span.textContent = text;
  toastEl.classList.add("is-on");
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => toastEl.classList.remove("is-on"), 2400);
}

// --- Update check ----------------------------------------------------------
function renderUpdateBar(): void {
  updateBarEl.innerHTML = updateBarHTML(state);
  updateBarEl.classList.toggle("is-on", !!state.update);
}

async function maybeCheckUpdate(): Promise<void> {
  if (!state.settings.checkUpdates) return;
  try {
    const version = await getVersion();
    const info: UpdateInfo | null = await checkUpdate(version);
    if (info) {
      state.update = info;
      renderUpdateBar();
    }
  } catch {
    /* the update check must not interfere with the app's operation */
  }
}

// --- Scanning --------------------------------------------------------------
function appendWell(html: string): void {
  const lines = document.getElementById("scanLines");
  if (!lines) return;
  const div = document.createElement("div");
  div.innerHTML = html;
  lines.insertBefore(div, lines.lastChild);
}
function setStage(active: number): void {
  document.querySelectorAll<HTMLElement>(".scan__stage").forEach((el, i) => {
    el.classList.toggle("is-active", i === active);
    el.classList.toggle("is-done", i < active);
  });
}
function startStageAnim(name: string): { stop(): void } {
  appendWell(`<span class="cy">$</span> dk-doctor ./${name}`);
  setStage(0);
  appendWell('<span class="dim">reading project files …</span>');
  const timers: number[] = [];
  timers.push(
    window.setTimeout(() => {
      setStage(1);
      appendWell('<span class="dim">building the project model …</span>');
    }, reduced ? 50 : 450),
  );
  timers.push(
    window.setTimeout(() => {
      setStage(2);
      appendWell('<span class="dim">checking for problems …</span>');
    }, reduced ? 100 : 900),
  );
  return {
    stop() {
      timers.forEach((id) => clearTimeout(id));
    },
  };
}
function animateCounts(res: ScanResult, dur: number): void {
  const targets: [string, number][] = [
    ["cMaps", res.stats.maps],
    ["cEvents", res.stats.events],
    ["cAssets", res.stats.assets],
    ["cPlugins", res.stats.plugins],
  ];
  const loc = state.lang === "ru" ? "ru-RU" : "en-US";
  const start = performance.now();
  const step = (now: number) => {
    const p = Math.min(1, (now - start) / dur);
    const e = 1 - Math.pow(1 - p, 3);
    for (const [id, target] of targets) {
      const el = document.getElementById(id);
      if (el) el.textContent = Math.round(target * e).toLocaleString(loc);
    }
    if (p < 1) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
}
async function finishScanAnim(res: ScanResult, started: number): Promise<void> {
  document.querySelectorAll<HTMLElement>(".scan__stage").forEach((el) => {
    el.classList.remove("is-active");
    el.classList.add("is-done");
  });
  const bar = document.getElementById("scanBar");
  if (bar) {
    bar.classList.remove("is-indeterminate");
    const fill = bar.firstElementChild as HTMLElement | null;
    if (fill) fill.style.width = "100%";
  }
  animateCounts(res, reduced ? 80 : 420);
  const su = res.report.summary;
  const cls = computeHealth(su).ring === "ok" ? "ok" : computeHealth(su).ring === "warn" ? "wn" : "er";
  appendWell(
    `done → health <span class="${cls}">${computeHealth(su).score}</span><span class="dim">/100</span> · ` +
      `<span class="er">${su.errors}</span> <span class="dim">err</span> ` +
      `<span class="wn">${su.warnings}</span> <span class="dim">warn</span> ` +
      `<span class="bl">${su.infos}</span> <span class="dim">info</span>`,
  );
  const MIN = reduced ? 200 : 750;
  const elapsed = Date.now() - started;
  if (elapsed < MIN) await sleep(MIN - elapsed);
  await sleep(reduced ? 60 : 380);
}

async function startScan(path: string): Promise<void> {
  if (busy) return;
  busy = true;
  const name = baseName(path);
  state.project = { path, name };
  state.view = "scanning";
  state.error = undefined;
  state.drawer = null;
  closeDrawer();
  closeSettings();
  closeOverlay();
  applyChrome();
  appEl.dataset.state = "scanning";
  appbarEl.innerHTML = appbarHTML(state);
  setView("view view--center", scanShellHTML(state));

  const started = Date.now();
  const anim = startStageAnim(name);
  try {
    const res = await scan(path, state.lang, {
      orphans: state.settings.orphans,
      deadCommonEvents: state.settings.deadCommonEvents,
    });
    anim.stop();
    await finishScanAnim(res, started);
    state.report = res.report;
    state.stats = res.stats;
    state.warnings = res.warnings;
    state.scannedAt = Date.now();
    state.filters = emptyFilters();
    state.expanded = new Set();
    state.ignored = new Set();
    state.drawer = null;
    state.newOnly = false;
    state.atlas = undefined;
    state.graph = undefined;
    graphLoading = false; // a stale in-flight fetch must not block the new project's graph
    state.atlasSel = null;
    state.atlasEvent = null;
    state.atlasNewOnly = false;
    state.atlasRegions = false;
    state.overlay = null;
    cmdContextCache.clear();
    // Diff against the previous run of this project, then make this run the new
    // baseline so the next run compares against it.
    const prevSnap = loadSnapshot(path);
    state.diff = diffReport(prevSnap ? prevSnap.fps : null, res.report);
    const score = computeHealth(res.report.summary).score;
    saveSnapshot(path, {
      ts: state.scannedAt,
      summary: res.report.summary,
      score,
      fps: fingerprintsOf(res.report),
    });
    // Record this run on the Time Machine timeline.
    state.history = appendHistory(path, {
      ts: state.scannedAt,
      score,
      errors: res.report.summary.errors,
      warnings: res.report.summary.warnings,
      infos: res.report.summary.infos,
    });
    state.recent = addRecent({
      path,
      name,
      engine: res.stats.engine,
      score,
      ts: state.scannedAt,
    });
    state.view = "report";
    paint();
    // (Re)arm Watch Mode for this project if the preference is on.
    if (state.settings.watch) void startWatch();
    // Fetch the render-only map geometry in the background; when it arrives, the
    // atlas re-renders to mount the spatial canvas. Failure is non-fatal — the
    // atlas falls back to a flat per-map list.
    void mapAtlas(path)
      .then((a) => {
        state.atlas = a;
        // Restore the last selection/region toggle remembered for this project.
        const saved = loadAtlasMemory(path);
        if (saved) {
          state.atlasRegions = saved.regions;
          if (saved.sel === "overview" || saved.sel === "project") {
            state.atlasSel = saved.sel;
          } else if (typeof saved.sel === "number" && a.some((m) => m.mapId === saved.sel)) {
            state.atlasSel = saved.sel;
          }
        }
        if (state.view === "report" && state.reportMode === "atlas") renderReportView();
      })
      .catch(() => {
        /* geometry is optional */
      });
  } catch (e) {
    anim.stop();
    state.error = errMessage(e);
    state.view = "error";
    paint();
  } finally {
    busy = false;
  }
}

async function pickAndScan(): Promise<void> {
  if (busy) return;
  try {
    const p = await pickFolder(t(state.lang, "dlgPickFolder"));
    if (p) void startScan(p);
  } catch (e) {
    // Cancellation returns null (handled above) — only real dialog failures
    // reach here; we show them to the user instead of swallowing them silently.
    toast(errMessage(e));
  }
}

// --- Report/settings actions ----------------------------------------------
function setGroupBy(g: GroupBy): void {
  if (state.groupBy === g) return;
  state.groupBy = g;
  renderReportView();
  syncDrawer();
}
function toggleFacet(group: "severity" | "category" | "confidence", val: string): void {
  const set = state.filters[group] as Set<string>;
  if (set.has(val)) set.delete(val);
  else set.add(val);
  renderReportView();
  syncDrawer();
}
function clearFacet(group: "severity" | "category" | "confidence"): void {
  (state.filters[group] as Set<string>).clear();
  renderReportView();
  syncDrawer();
}
function toggleTile(cat: string): void {
  const set = state.filters.category as Set<string>;
  if (set.size === 1 && set.has(cat)) set.clear();
  else {
    set.clear();
    set.add(cat);
  }
  renderReportView();
  syncDrawer();
}
function toggleAgg(key: string, expand: boolean): void {
  if (expand) state.expanded.add(key);
  else state.expanded.delete(key);
  repaintMain();
}

function drawerAction(act: string): void {
  if (state.drawer === null || !state.report) return;
  const f = state.report.findings[state.drawer];
  if (!f) return;
  if (act === "copy") {
    const text = f.path ? `${f.file} ${f.path}` : f.file;
    const clip = navigator.clipboard;
    if (clip) {
      clip
        .writeText(text)
        .then(() => toast(t(state.lang, "copied")))
        .catch(() => toast(t(state.lang, "copyFailed")));
    } else {
      toast(t(state.lang, "copyFailed"));
    }
  } else if (act === "ignore") {
    state.ignored.add(state.drawer);
    closeDrawer();
    renderReportView();
    toast(t(state.lang, "ignored"));
  }
}

function doExport(): void {
  if (!state.report) return;
  const safe = (state.project?.name ?? "health").replace(/[^\w.-]+/g, "_");
  exportReport(state.report, `${safe}_report.json`, t(state.lang, "dlgSaveReport"))
    .then((ok) => {
      if (ok) toast(t(state.lang, "exported"));
    })
    .catch(() => toast(t(state.lang, "exportFailed")));
}

/** Project name reduced to a filesystem-safe stem (shared by exporters). */
function safeStem(): string {
  return (state.project?.name ?? "health").replace(/[^\w.-]+/g, "_");
}

/** Exports the report as a standalone, printable HTML document (D5). */
async function doExportHtml(): Promise<void> {
  if (!state.report) return;
  const html = buildReportHtml({
    report: state.report,
    stats: state.stats,
    projectName: state.project?.name ?? "",
    projectPath: state.project?.path ?? "",
    lang: state.lang,
    generatedAt: Date.now(),
  });
  try {
    const ok = await saveTextFile(html, `${safeStem()}_report.html`, t(state.lang, "dlgSaveHtml"), {
      name: "HTML",
      extensions: ["html"],
    });
    if (ok) toast(t(state.lang, "exported"));
  } catch {
    toast(t(state.lang, "exportFailed"));
  }
}

/** Exports a shareable PNG summary card (D10). */
async function doExportCard(): Promise<void> {
  if (!state.report) return;
  const url = renderSummaryCard({
    report: state.report,
    stats: state.stats,
    projectName: state.project?.name ?? "",
    lang: state.lang,
    generatedAt: Date.now(),
  });
  if (!url) {
    toast(t(state.lang, "pngFailed"));
    return;
  }
  try {
    const ok = await saveImagePng(url, `${safeStem()}_card.png`, t(state.lang, "dlgSavePng"));
    if (ok) toast(t(state.lang, "pngSaved"));
  } catch {
    toast(t(state.lang, "pngFailed"));
  }
}

/** Dispatches an export chosen from the export overlay. */
function exportAs(kind: string): void {
  closeOverlay();
  if (kind === "html") void doExportHtml();
  else if (kind === "png") void doExportCard();
  else doExport();
}

// --- Watch Mode ------------------------------------------------------------
/**
 * Begins watching the open project for changes (replacing any prior watcher).
 *
 * `startWatch`/`stopWatch` overlap: the async `listen`/`watchProject` round-trips
 * can interleave with a toggle-off or a project switch. A monotonic token guards
 * against that — if it advances during an await, the just-established listener is
 * torn down instead of silently re-arming a watcher the user disabled.
 */
async function startWatch(): Promise<void> {
  if (!state.project) return;
  stopWatch();
  const path = state.project.path;
  const token = ++watchToken;
  try {
    const unlisten = await onProjectChanged((changedPath) => {
      // Guard on the live preference too: a stale listener must never re-scan
      // after Watch Mode was turned off.
      if (!state.settings.watch || busy || state.view !== "report") return;
      if (!state.project || state.project.path !== changedPath) return;
      // Coalesce a burst of file writes into one re-scan.
      if (watchTimer) clearTimeout(watchTimer);
      watchTimer = window.setTimeout(() => {
        if (!busy && state.project?.path === changedPath) void startScan(changedPath);
      }, 400);
    });
    if (token !== watchToken) {
      unlisten();
      return;
    }
    watchUnlisten = unlisten;
    await watchProject(path);
    // A toggle-off / re-arm that happened while awaiting invalidates us.
    if (token !== watchToken) stopWatch();
  } catch {
    /* watch unavailable (outside Tauri / permission) — the toggle is a no-op */
  }
}

/** Stops watching and clears the pending debounce (idempotent). */
function stopWatch(): void {
  watchToken++; // invalidate any startWatch still awaiting its listen/watch calls
  if (watchTimer) {
    clearTimeout(watchTimer);
    watchTimer = undefined;
  }
  if (watchUnlisten) {
    watchUnlisten();
    watchUnlisten = null;
  }
  void unwatchProject().catch(() => {});
}

/** Toggles Watch Mode from the toolbar (persists the preference). */
function toggleWatch(): void {
  const on = !state.settings.watch;
  state.settings = { ...state.settings, watch: on };
  saveSettings(state.settings);
  if (on) {
    void startWatch();
    toast(t(state.lang, "watchEnabled"));
  } else {
    stopWatch();
    toast(t(state.lang, "watchDisabled"));
  }
  if (state.view === "report") renderReportView();
}

function clearRecentList(): void {
  clearRecent();
  clearSnapshots();
  state.recent = [];
  if (state.view === "welcome") setView("view view--welcome", welcomeHTML(state));
  toast(t(state.lang, "recentCleared"));
}

// --- Event delegation ------------------------------------------------------
document.addEventListener("click", (e) => {
  const target = e.target as HTMLElement;

  // removing a recent project (before data-open, since it's nested inside it)
  const del = target.closest<HTMLElement>("[data-del]");
  if (del) {
    e.stopPropagation();
    removeSnapshot(del.dataset.del!);
    removeHistory(del.dataset.del!);
    state.recent = removeRecent(del.dataset.del!);
    if (state.view === "welcome") setView("view view--welcome", welcomeHTML(state));
    return;
  }

  const open = target.closest<HTMLElement>("[data-open]");
  if (open) {
    void startScan(open.dataset.open!);
    return;
  }

  const actEl = target.closest<HTMLElement>("[data-act]");
  if (actEl) {
    const act = actEl.dataset.act!;
    switch (act) {
      case "pick":
        void pickAndScan();
        break;
      case "toggle-theme":
        applySettings({ theme: state.theme === "dark" ? "light" : "dark" });
        break;
      case "open-settings":
        openSettings();
        break;
      case "close-settings":
        closeSettings();
        break;
      case "rerun":
        if (state.project) void startScan(state.project.path);
        break;
      case "toggle-new":
        state.newOnly = !state.newOnly;
        repaintMain();
        syncDrawer();
        break;
      case "atlas-newonly":
        state.atlasNewOnly = !state.atlasNewOnly;
        renderReportView();
        break;
      case "open-export":
        openOverlay("export");
        break;
      case "open-readiness":
        openOverlay("readiness");
        break;
      case "open-timeline":
        openOverlay("timeline");
        break;
      case "close-overlay":
        closeOverlay();
        break;
      case "toggle-watch":
        toggleWatch();
        break;
      case "copy":
      case "ignore":
        drawerAction(act);
        break;
      case "open-docs":
        if (actEl.dataset.doc) void openRelease(actEl.dataset.doc);
        break;
      case "clear-recent":
        clearRecentList();
        break;
      case "get-update":
        if (state.update) void openRelease(state.update.url);
        break;
      case "dismiss-update":
        state.update = undefined;
        renderUpdateBar();
        break;
    }
    return;
  }

  // "show on map" jump from a finding row (must win over the row's data-find)
  const showMap = target.closest<HTMLElement>("[data-showmap]");
  if (showMap) {
    const [m, ev] = (showMap.dataset.showmap ?? "").split(":");
    const mapId = Number(m);
    if (!Number.isNaN(mapId)) {
      state.reportMode = "atlas";
      state.atlasSel = mapId;
      state.atlasEvent = ev ? Number(ev) : null;
      pendingFocus = state.atlasEvent;
      persistAtlas();
      renderReportView();
      syncDrawer();
    }
    return;
  }

  // finding row
  const find = target.closest<HTMLElement>("[data-find]");
  if (find) {
    openDrawer(Number(find.dataset.find));
    return;
  }
  const exp = target.closest<HTMLElement>("[data-expand]");
  if (exp) {
    toggleAgg(exp.dataset.expand!, true);
    return;
  }
  const col = target.closest<HTMLElement>("[data-collapse]");
  if (col) {
    toggleAgg(col.dataset.collapse!, false);
    return;
  }
  const tile = target.closest<HTMLElement>("[data-tile]");
  if (tile) {
    toggleTile(tile.dataset.tile!);
    return;
  }
  const facet = target.closest<HTMLElement>("[data-facet]");
  if (facet) {
    toggleFacet(
      facet.dataset.facet as "severity" | "category" | "confidence",
      facet.dataset.val!,
    );
    return;
  }
  const clr = target.closest<HTMLElement>("[data-clearfacet]");
  if (clr) {
    clearFacet(clr.dataset.clearfacet as "severity" | "category" | "confidence");
    return;
  }
  const seg = target.closest<HTMLElement>("[data-seg]");
  if (seg) {
    setGroupBy(seg.dataset.seg as GroupBy);
    return;
  }
  const modeBtn = target.closest<HTMLElement>("[data-mode]");
  if (modeBtn) {
    setReportMode(modeBtn.dataset.mode as "atlas" | "list" | "graph");
    return;
  }
  // export chooser option (in the export overlay)
  const exBtn = target.closest<HTMLElement>("[data-export]");
  if (exBtn) {
    exportAs(exBtn.dataset.export!);
    return;
  }
  const treeTog = target.closest<HTMLElement>("[data-treetoggle]");
  if (treeTog) {
    const key = treeTog.dataset.treetoggle!;
    if (state.expanded.has(key)) state.expanded.delete(key);
    else state.expanded.add(key);
    renderReportView();
    return;
  }
  const mapSel = target.closest<HTMLElement>("[data-mapsel]");
  if (mapSel) {
    const v = mapSel.dataset.mapsel!;
    // A click on a graph node jumps to that map's atlas view.
    if (state.reportMode === "graph") state.reportMode = "atlas";
    selectAtlas(v === "project" || v === "overview" ? v : Number(v));
    return;
  }

  // drawer: prev/next/close
  const nav = target.closest<HTMLElement>("[data-nav]");
  if (nav) {
    const d = nav.dataset.nav;
    if (d === "close") closeDrawer();
    else if (d === "prev") navDrawer(-1);
    else navDrawer(1);
    return;
  }

  // settings: segmented controls/toggles
  const setBtn = target.closest<HTMLElement>("[data-set]");
  if (setBtn) {
    const key = setBtn.dataset.set as "theme" | "density" | "lang";
    applySettings({ [key]: setBtn.dataset.val } as Partial<Settings>);
    return;
  }
  const tog = target.closest<HTMLElement>("[data-toggle]");
  if (tog) {
    const key = tog.dataset.toggle as "orphans" | "deadCommonEvents" | "checkUpdates";
    applySettings({ [key]: !state.settings[key] } as Partial<Settings>);
    return;
  }

  if (target === scrimEl) closeDrawer();
  else if (target === settingsScrimEl) closeSettings();
  else if (target === overlayScrimEl) closeOverlay();
});


document.addEventListener("keydown", (e) => {
  const target = e.target as HTMLElement;
  const typing = /^(INPUT|TEXTAREA)$/.test(target.tagName);

  if (e.key === "Escape") {
    if (settingsEl.classList.contains("is-open")) closeSettings();
    else if (state.overlay !== null) closeOverlay();
    else if (state.drawer !== null) closeDrawer();
    return;
  }
  // activating a recent project from the keyboard
  if ((e.key === "Enter" || e.key === " ") && target.dataset && target.dataset.open) {
    e.preventDefault();
    void startScan(target.dataset.open);
    return;
  }
  if (state.drawer !== null && !typing) {
    if (e.key === "j" || e.key === "ArrowDown") {
      e.preventDefault();
      navDrawer(1);
    } else if (e.key === "k" || e.key === "ArrowUp") {
      e.preventDefault();
      navDrawer(-1);
    }
  }
});

// the system theme changes on the fly
mql.addEventListener("change", () => {
  if (state.settings.theme === "system") {
    applyChrome();
    appbarEl.innerHTML = appbarHTML(state);
  }
});

// drag-and-drop of a project folder into the window (native Tauri channel)
function registerDragDrop(): void {
  try {
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        const drop = document.getElementById("drop");
        const p = event.payload;
        if (p.type === "over" || p.type === "enter") {
          if (state.view === "welcome" && drop) drop.classList.add("is-over");
        } else if (p.type === "drop") {
          if (drop) drop.classList.remove("is-over");
          const path = p.paths && p.paths[0];
          if (path && !busy && state.view !== "scanning") void startScan(path);
        } else if (drop) {
          drop.classList.remove("is-over");
        }
      })
      .catch(() => {});
  } catch {
    /* outside Tauri (e.g. vite preview) — drag-drop is unavailable */
  }
}
registerDragDrop();

// --- Start -----------------------------------------------------------------
try {
  paint();
} catch (e) {
  console.error(e);
}
// The window is created hidden (visible:false) — we show it after the first
// frame, even if paint above threw, so the user doesn't see emptiness instead
// of the app. getCurrentWindow() synchronously dereferences __TAURI_INTERNALS__
// → outside Tauri (vite preview) it throws synchronously, so we wrap it in try.
// The backend also shows the window on a timeout if the frontend never got here.
try {
  void getCurrentWindow()
    .show()
    .catch(() => {});
} catch {
  /* outside Tauri the window is already visible */
}

// Unobtrusive update check on startup (if enabled in the settings).
void maybeCheckUpdate();
