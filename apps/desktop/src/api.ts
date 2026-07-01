import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";

/** Severity level of a finding (as in serde: lowercase). */
export type Severity = "error" | "warning" | "info";
/** Confidence of a finding: certain (static analysis) or likely (heuristic). */
export type Confidence = "certain" | "likely";
/** Category of a finding (as in serde: kebab-case). */
export type Category =
  | "data"
  | "reference"
  | "asset"
  | "dead-code"
  | "plugin-order"
  | "plugin-conflict";
/** Output language. */
export type Lang = "ru" | "en";
/** Project engine. */
export type EngineId = "mv" | "mz";

/** A related site of a finding. */
export interface Reference {
  file: string;
  path: string;
}

/** A single finding from the JSON artifact (mirror of `report_json.rs`). */
export interface Finding {
  rule: string;
  severity: Severity;
  category: Category;
  confidence: Confidence;
  /** Stable, language-neutral finding identity (rule + file + path + args). */
  fingerprint: string;
  file: string;
  path: string;
  message_key: string;
  /** Msg is marked `#[serde(tag="key")]`, so `args.key` is always present. */
  args: { key: string } & Record<string, unknown>;
  /** Ready-made localized text — displayed verbatim. */
  message: string;
  references: Reference[];
}

/** Summary by severity level. */
export interface Summary {
  errors: number;
  warnings: number;
  infos: number;
}

/** Full analyzer report. */
export interface Report {
  engine: EngineId;
  lang: Lang;
  summary: Summary;
  findings: Finding[];
}

/** Aggregate project statistics (mirror of `ProjectStats` on the Rust side). */
export interface ProjectStats {
  engine: EngineId;
  maps: number;
  events: number;
  commands: number;
  plugins: number;
  assets: number;
}

/** Scan result: statistics + parsed report + load warnings. */
export interface ScanResult {
  stats: ProjectStats;
  report: Report;
  /** Project files that could not be parsed (skipped). */
  warnings: string[];
}

/** First-page graphic of an event (mirror of Rust `EventGraphic`). */
export interface EventGraphic {
  /** Character sheet stem (img/characters/<name>.png); empty for a tile graphic. */
  characterName: string;
  /** Sub-character index (0..7) in a normal 8-char sheet. */
  characterIndex: number;
  /** Facing direction (2=down, 4=left, 6=right, 8=up). */
  direction: number;
  /** Walk frame / pattern (0..2). */
  pattern: number;
  /** Tile id when the event is drawn as a tileset tile (>0), else 0. */
  tileId: number;
}

/** One event placed on a map grid (mirror of Rust `AtlasEvent`). */
export interface AtlasEvent {
  id: number;
  name: string;
  x: number;
  y: number;
  /** First-page graphic; `null` when the event is invisible. */
  graphic: EventGraphic | null;
}

/** One map's schematic geometry (mirror of Rust `MapAtlas`). */
export interface MapAtlas {
  mapId: number;
  name: string;
  parentId: number;
  width: number;
  height: number;
  events: AtlasEvent[];
}

/** Full tile render data for one map (mirror of Rust `MapRender`). */
export interface MapRender {
  width: number;
  height: number;
  tilesetId: number;
  /** tilesetNames[9] = [A1,A2,A3,A4,A5,B,C,D,E] image stems (may be empty). */
  tilesetNames: string[];
  /** Flat layered tile-id array (length = width*height*6). */
  data: number[];
}

/** Analysis options (mirror of `AnalyzeOpts`; camelCase under serde-rename). */
export interface AnalyzeOpts {
  /** Enable the opt-in `orphan-assets` rule. */
  orphans?: boolean;
  /** Enable the opt-in `dead-common-event` rule. */
  deadCommonEvents?: boolean;
}

/** Raw response of the `scan` command (report is still a JSON string). */
interface RawScan {
  stats: ProjectStats;
  report: string;
  warnings: string[];
}

/**
 * Opens the system folder picker; returns the path or `null` on cancel.
 * `title` is the localized dialog title (passed by the calling code).
 */
export async function pickFolder(title: string): Promise<string | null> {
  const sel = await open({
    directory: true,
    multiple: false,
    title,
  });
  return typeof sel === "string" ? sel : null;
}

/**
 * Scans the project: the `scan` command loads it once and returns statistics
 * + the JSON report (identical to `dk-doctor --format json`). Throws an error
 * string on load failure (no `data/`, encrypted, not RPG Maker).
 */
export async function scan(
  path: string,
  lang: Lang,
  opts: AnalyzeOpts = {},
): Promise<ScanResult> {
  const raw = await invoke<RawScan>("scan", { path, lang, opts });
  return {
    stats: raw.stats,
    report: JSON.parse(raw.report) as Report,
    warnings: raw.warnings ?? [],
  };
}

/**
 * Fetches the render-only map geometry sidecar (per-map size + event tile
 * coordinates) for the Atlas view. Independent of the findings report; the
 * caller joins findings to events by their location path. May reject — callers
 * should treat failure as "no geometry" and fall back to the flat list.
 */
export async function mapAtlas(path: string): Promise<MapAtlas[]> {
  return await invoke<MapAtlas[]>("map_atlas", { path });
}

/** Fetches the full tile render data for one map (geometry + tiles + tileset). */
export async function mapRender(path: string, mapId: number): Promise<MapRender> {
  return await invoke<MapRender>("map_render", { path, mapId });
}

/**
 * Reads a project image (relative to the asset root, e.g.
 * `img/tilesets/World.png`) as raw bytes. Returns an `ArrayBuffer` suitable for
 * `createImageBitmap(new Blob([buf]))`. Rejects if the file is missing/encrypted.
 */
export async function readProjectImage(path: string, rel: string): Promise<ArrayBuffer> {
  return await invoke<ArrayBuffer>("read_project_image", { root: path, rel });
}

/** One command line of an event page (mirror of Rust `CommandLine`). */
export interface CommandLine {
  index: number;
  indent: number;
  code: number;
  /** Language-neutral key arg (switch/var/CE id, self-switch ch), or empty. */
  arg: string;
}

/** Fetches one event page's command list for the finding "context" view. */
export async function eventCommands(
  path: string,
  mapId: number,
  eventId: number,
  page: number,
): Promise<CommandLine[]> {
  return await invoke<CommandLine[]>("event_commands", { path, mapId, eventId, page });
}

/**
 * Exports the report to a JSON file: system save dialog → write via the
 * `write_text_file` command. Returns `false` if the user cancelled.
 */
export async function exportReport(
  report: Report,
  defaultName: string,
  title: string,
): Promise<boolean> {
  const path = await save({
    title,
    defaultPath: defaultName,
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (!path) return false;
  await invoke("write_text_file", {
    path,
    contents: JSON.stringify(report, null, 2),
  });
  return true;
}

/** Decodes a `data:image/png;base64,…` URL into raw bytes. */
function dataUrlToBytes(url: string): Uint8Array {
  const b64 = url.slice(url.indexOf(",") + 1);
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/**
 * Saves a PNG data URL to a user-chosen file: system save dialog → write the
 * decoded bytes via the `write_binary_file` command. Returns `false` on cancel.
 */
export async function saveImagePng(
  dataUrl: string,
  defaultName: string,
  title: string,
): Promise<boolean> {
  const path = await save({
    title,
    defaultPath: defaultName,
    filters: [{ name: "PNG", extensions: ["png"] }],
  });
  if (!path) return false;
  await invoke("write_binary_file", {
    path,
    bytes: Array.from(dataUrlToBytes(dataUrl)),
  });
  return true;
}

/** Language from the OS locale (fallback is `en`). */
export function systemLang(): Lang {
  return (navigator.language || "en").toLowerCase().startsWith("ru")
    ? "ru"
    : "en";
}

/** Info about an available update (new version + link to the release). */
export interface UpdateInfo {
  /** Version of the latest release (without the `v` prefix). */
  version: string;
  /** Link to the release page on GitHub. */
  url: string;
}

const RELEASES_API =
  "https://api.github.com/repos/DKPlugins/DK-Doctor/releases?per_page=5";
const RELEASES_PAGE = "https://github.com/DKPlugins/DK-Doctor/releases";

/** Compares versions `a > b` by the numeric components `x.y.z`. */
function isNewer(a: string, b: string): boolean {
  const pa = a.split(".").map((n) => parseInt(n, 10) || 0);
  const pb = b.split(".").map((n) => parseInt(n, 10) || 0);
  for (let i = 0; i < 3; i++) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (x !== y) return x > y;
  }
  return false;
}

/**
 * Asks GitHub Releases for the latest version and compares it with `current`.
 * Returns `null` if there is no update or the request failed (silently — the
 * check must not interfere with work). The network request is allowed by the
 * CSP only to api.github.com.
 */
export async function checkUpdate(current: string): Promise<UpdateInfo | null> {
  try {
    const res = await fetch(RELEASES_API, {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!res.ok) return null;
    const list = (await res.json()) as Array<{
      tag_name?: string;
      html_url?: string;
      draft?: boolean;
    }>;
    const rel = list.find((r) => r && r.tag_name && !r.draft);
    if (!rel?.tag_name) return null;
    const latest = rel.tag_name.replace(/^v/i, "");
    if (!isNewer(latest, current)) return null;
    return { version: latest, url: rel.html_url ?? RELEASES_PAGE };
  } catch {
    return null;
  }
}

/** Opens the release link in the system browser (via the backend). */
export async function openRelease(url: string): Promise<void> {
  await invoke("open_url", { url });
}
