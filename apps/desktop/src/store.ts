import type { EngineId, Lang, Summary } from "./api";

/**
 * Persistence of settings and recent projects via `localStorage`. The Tauri
 * WebView (WebView2/WKWebView) stores it in the app profile across launches;
 * everything is wrapped in try/catch — corrupted/unavailable storage does not break the UI.
 */

/** Theme preference. */
export type ThemePref = "system" | "light" | "dark";
/** Language preference. */
export type LangPref = "system" | Lang;
/** Interface density. */
export type DensityPref = "comfortable" | "compact";

/** Persisted UI + analysis settings. */
export interface Settings {
  theme: ThemePref;
  lang: LangPref;
  density: DensityPref;
  /** Enable the opt-in `orphan-assets` rule (off by default). */
  orphans: boolean;
  /** Enable the opt-in `dead-common-event` rule (off by default). */
  deadCommonEvents: boolean;
  /** Check for new versions on GitHub at startup (the only network request). */
  checkUpdates: boolean;
}

/** Default settings (theme/language — system, opt-in rules off). */
export const defaultSettings = (): Settings => ({
  theme: "system",
  lang: "system",
  density: "comfortable",
  orphans: false,
  deadCommonEvents: false,
  checkUpdates: true,
});

/** A recent-project record. */
export interface RecentProject {
  /** Absolute path to the project folder. */
  path: string;
  /** Display name (folder name). */
  name: string;
  /** Engine of the last analysis. */
  engine: EngineId;
  /** Health score of the last analysis (0..100). */
  score: number;
  /** Timestamp of the last analysis (ms epoch). */
  ts: number;
}

const SETTINGS_KEY = "dkd.settings.v1";
const RECENT_KEY = "dkd.recent.v1";
const SNAPSHOT_KEY = "dkd.snapshots.v1";
const ATLAS_KEY = "dkd.atlas.v1";
/** How many projects keep a remembered atlas view (oldest evicted). */
const ATLAS_MAX_PROJECTS = 24;
const RECENT_MAX = 8;
/** How many projects keep a run snapshot for diffing (oldest evicted). */
const SNAPSHOT_MAX_PROJECTS = 24;
/** Cap on stored fingerprints per project (huge reports stay bounded). */
const SNAPSHOT_MAX_FPS = 6000;

function readJson(key: string): unknown {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as unknown) : null;
  } catch {
    return null;
  }
}

function writeJson(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* private mode / quota exceeded — silently ignore */
  }
}

const THEMES: ThemePref[] = ["system", "light", "dark"];
const LANGS: LangPref[] = ["system", "ru", "en"];
const DENSITIES: DensityPref[] = ["comfortable", "compact"];

/** Loads settings, validating each field against corrupted storage. */
export function loadSettings(): Settings {
  const d = defaultSettings();
  const raw = readJson(SETTINGS_KEY);
  if (!raw || typeof raw !== "object") return d;
  const r = raw as Record<string, unknown>;
  return {
    theme: THEMES.includes(r.theme as ThemePref) ? (r.theme as ThemePref) : d.theme,
    lang: LANGS.includes(r.lang as LangPref) ? (r.lang as LangPref) : d.lang,
    density: DENSITIES.includes(r.density as DensityPref)
      ? (r.density as DensityPref)
      : d.density,
    orphans: typeof r.orphans === "boolean" ? r.orphans : d.orphans,
    deadCommonEvents:
      typeof r.deadCommonEvents === "boolean" ? r.deadCommonEvents : d.deadCommonEvents,
    checkUpdates:
      typeof r.checkUpdates === "boolean" ? r.checkUpdates : d.checkUpdates,
  };
}

/** Saves settings. */
export function saveSettings(s: Settings): void {
  writeJson(SETTINGS_KEY, s);
}

/** Validates a single recent-project record. */
function isRecent(x: unknown): x is RecentProject {
  if (!x || typeof x !== "object") return false;
  const r = x as Record<string, unknown>;
  return (
    typeof r.path === "string" &&
    typeof r.name === "string" &&
    (r.engine === "mv" || r.engine === "mz") &&
    typeof r.score === "number" &&
    typeof r.ts === "number"
  );
}

/** Loads the list of recent projects (filtered of garbage). */
export function loadRecent(): RecentProject[] {
  const raw = readJson(RECENT_KEY);
  if (!Array.isArray(raw)) return [];
  return raw.filter(isRecent).slice(0, RECENT_MAX);
}

/**
 * Adds/updates a project at the head of the list (dedup by path), trims to the
 * limit, saves, and returns the new list.
 */
export function addRecent(p: RecentProject): RecentProject[] {
  const rest = loadRecent().filter((r) => r.path !== p.path);
  const list = [p, ...rest].slice(0, RECENT_MAX);
  writeJson(RECENT_KEY, list);
  return list;
}

/** Removes a project by path; returns the new list. */
export function removeRecent(path: string): RecentProject[] {
  const list = loadRecent().filter((r) => r.path !== path);
  writeJson(RECENT_KEY, list);
  return list;
}

/** Fully clears the list of recent projects. */
export function clearRecent(): void {
  writeJson(RECENT_KEY, []);
}

/**
 * Snapshot of a single analyzer run, kept to diff the next run against it
 * ("what changed since last time"). `fps` are language-neutral fingerprints of
 * the findings (see `group.fingerprint`). Stored per project path.
 */
export interface RunSnapshot {
  /** Timestamp of the run (ms epoch). */
  ts: number;
  /** Summary by severity at that run. */
  summary: Summary;
  /** Health score at that run (0..100). */
  score: number;
  /** Finding fingerprints of that run. */
  fps: string[];
}

/** Validates a stored snapshot record. */
function isSnapshot(x: unknown): x is RunSnapshot {
  if (!x || typeof x !== "object") return false;
  const r = x as Record<string, unknown>;
  const s = r.summary as Record<string, unknown> | undefined;
  return (
    typeof r.ts === "number" &&
    typeof r.score === "number" &&
    Array.isArray(r.fps) &&
    !!s &&
    typeof s.errors === "number" &&
    typeof s.warnings === "number" &&
    typeof s.infos === "number"
  );
}

/** Reads the whole snapshot map (path → snapshot), filtered of garbage. */
function loadSnapshots(): Record<string, RunSnapshot> {
  const raw = readJson(SNAPSHOT_KEY);
  if (!raw || typeof raw !== "object") return {};
  const out: Record<string, RunSnapshot> = {};
  for (const [path, snap] of Object.entries(raw as Record<string, unknown>)) {
    if (isSnapshot(snap)) out[path] = snap;
  }
  return out;
}

/** Snapshot of the previous run for `path` (or `null` if this is the first run). */
export function loadSnapshot(path: string): RunSnapshot | null {
  return loadSnapshots()[path] ?? null;
}

/**
 * Stores the snapshot for `path` (overwriting the previous run), capping the
 * fingerprint list and evicting the oldest projects beyond the limit.
 */
export function saveSnapshot(path: string, snap: RunSnapshot): void {
  const map = loadSnapshots();
  map[path] = { ...snap, fps: snap.fps.slice(0, SNAPSHOT_MAX_FPS) };
  const entries = Object.entries(map).sort((a, b) => b[1].ts - a[1].ts);
  writeJson(SNAPSHOT_KEY, Object.fromEntries(entries.slice(0, SNAPSHOT_MAX_PROJECTS)));
}

/** Removes the snapshot for a single project path. */
export function removeSnapshot(path: string): void {
  const map = loadSnapshots();
  if (path in map) {
    delete map[path];
    writeJson(SNAPSHOT_KEY, map);
  }
}

/** Drops all stored run snapshots. */
export function clearSnapshots(): void {
  writeJson(SNAPSHOT_KEY, {});
}

// ===========================================================================
// Atlas view memory — selected map + viewport, remembered per project path
// ===========================================================================

/** Saved canvas viewport (pan offset + tile size) of the last opened map. */
export interface AtlasViewport {
  /** Map the viewport belongs to. */
  mapId: number;
  /** Pan offset x (px). */
  ox: number;
  /** Pan offset y (px). */
  oy: number;
  /** Tile size (px). */
  cell: number;
}

/** Remembered atlas state for one project (selection + viewport + toggles). */
export interface AtlasMemory {
  /** Selected target: a map id, the project board, or the overview heatmap. */
  sel: number | "project" | "overview";
  /** Viewport of the last opened map (null = none saved yet). */
  viewport: AtlasViewport | null;
  /** Region overlay toggle. */
  regions: boolean;
}

/** Validates a stored viewport record (rejects NaN/Infinity). */
function isViewport(x: unknown): x is AtlasViewport {
  if (!x || typeof x !== "object") return false;
  const r = x as Record<string, unknown>;
  return (
    Number.isFinite(r.mapId) &&
    Number.isFinite(r.ox) &&
    Number.isFinite(r.oy) &&
    Number.isFinite(r.cell)
  );
}

/** Validates a stored atlas-memory record. */
function isAtlasMemory(x: unknown): x is AtlasMemory {
  if (!x || typeof x !== "object") return false;
  const r = x as Record<string, unknown>;
  const selOk =
    Number.isFinite(r.sel) || r.sel === "project" || r.sel === "overview";
  return selOk && typeof r.regions === "boolean" && (r.viewport === null || isViewport(r.viewport));
}

/** Reads the whole atlas-memory map (path → memory), filtered of garbage. */
function loadAtlasMemories(): Record<string, AtlasMemory> {
  const raw = readJson(ATLAS_KEY);
  if (!raw || typeof raw !== "object") return {};
  const out: Record<string, AtlasMemory> = {};
  for (const [path, mem] of Object.entries(raw as Record<string, unknown>)) {
    if (isAtlasMemory(mem)) out[path] = mem;
  }
  return out;
}

/** Remembered atlas state for `path` (or `null` if none stored). */
export function loadAtlasMemory(path: string): AtlasMemory | null {
  return loadAtlasMemories()[path] ?? null;
}

/**
 * Stores the atlas memory for `path`, evicting the oldest projects beyond the
 * limit (insertion order — the freshly-saved path moves to the end).
 */
export function saveAtlasMemory(path: string, mem: AtlasMemory): void {
  const map = loadAtlasMemories();
  delete map[path];
  map[path] = mem;
  const entries = Object.entries(map);
  writeJson(ATLAS_KEY, Object.fromEntries(entries.slice(-ATLAS_MAX_PROJECTS)));
}
