import type { Category, Confidence, Finding, Report, Severity } from "./api";

/** Grouping axis for the findings list. */
export type GroupBy = "severity" | "category" | "map";

/** Active filters (empty set = "everything"). */
export interface Filters {
  severity: Set<Severity>;
  category: Set<Category>;
  confidence: Set<Confidence>;
}

/** Empty set of filters. */
export const emptyFilters = (): Filters => ({
  severity: new Set(),
  category: new Set(),
  confidence: new Set(),
});

/** A finding together with its index in `report.findings` (stable id). */
export interface IFinding {
  i: number;
  f: Finding;
}

/** Summary counters (by severity/category/confidence + breakdown). */
export interface Totals {
  sev: Record<Severity, number>;
  cat: Record<Category, number>;
  conf: Record<Confidence, number>;
  catBreak: Record<Category, Record<Severity, number>>;
  all: number;
}

/** Group of findings by the active axis. */
export interface Group {
  groupBy: GroupBy;
  /** Severity (for groupBy=severity) — sets the color/icon. */
  sev?: Severity;
  /** Category (for groupBy=category). */
  cat?: Category;
  /** Group name for the header (map/"Plugins"/"Database" when groupBy=map). */
  name: string;
  items: IFinding[];
}

/** Block of identical rules within a group (for pattern aggregation). */
export interface Block {
  rule: string;
  items: IFinding[];
}

export const SEVERITIES: Severity[] = ["error", "warning", "info"];
export const CATEGORIES: Category[] = [
  "data",
  "reference",
  "asset",
  "dead-code",
  "plugin-order",
  "plugin-conflict",
];
/** The analyzer distinguishes two confidence levels: certain (static) and likely (heuristic). */
export const CONFIDENCES: Confidence[] = ["certain", "likely"];

const SEV_ORDER: Severity[] = ["error", "warning", "info"];

/** Aggregation threshold: a rule with ≥ this many findings in a group is collapsed. */
export const AGG_THRESHOLD = 4;

/** Whether a finding passes the filters (severity/category/confidence). */
function passes(f: Finding, x: Filters): boolean {
  if (x.severity.size && !x.severity.has(f.severity)) return false;
  if (x.category.size && !x.category.has(f.category)) return false;
  if (x.confidence.size && !x.confidence.has(f.confidence)) return false;
  return true;
}

/** Whether a finding is not hidden (the "ignore" drawer). */
const isLive = (i: number, ignored: Set<number>) => !ignored.has(i);

/** Computes summary counters over NON-hidden findings (filters ignored). */
export function computeTotals(report: Report, ignored: Set<number>): Totals {
  const sev: Record<Severity, number> = { error: 0, warning: 0, info: 0 };
  const conf: Record<Confidence, number> = { certain: 0, likely: 0 };
  const cat = {} as Record<Category, number>;
  const catBreak = {} as Record<Category, Record<Severity, number>>;
  for (const c of CATEGORIES) {
    cat[c] = 0;
    catBreak[c] = { error: 0, warning: 0, info: 0 };
  }
  let all = 0;
  report.findings.forEach((f, i) => {
    if (!isLive(i, ignored)) return;
    sev[f.severity] += 1;
    conf[f.confidence] += 1;
    if (cat[f.category] !== undefined) {
      cat[f.category] += 1;
      catBreak[f.category][f.severity] += 1;
    }
    all += 1;
  });
  return { sev, cat, conf, catBreak, all };
}

/**
 * Visible findings (filters + not hidden), order from the report. When
 * `restrict` is given, only findings whose index is in the set pass (used by the
 * "new since last run" diff filter).
 */
export function visible(
  report: Report,
  filters: Filters,
  ignored: Set<number>,
  restrict?: Set<number> | null,
): IFinding[] {
  const out: IFinding[] = [];
  report.findings.forEach((f, i) => {
    if (restrict && !restrict.has(i)) return;
    if (isLive(i, ignored) && passes(f, filters)) out.push({ i, f });
  });
  return out;
}

/** Root group for the "Map" axis: map / Plugins / Assets / Database / file. */
export function mapGroupKey(f: Finding): string {
  const seg = f.path.split("/").filter(Boolean)[0] ?? "";
  const m = seg.match(/^Map\d+/i);
  if (m) return m[0];
  if (f.file.startsWith("js/plugins")) return "Plugins";
  if (f.file.startsWith("img") || f.file.startsWith("audio")) return "Assets";
  if (f.file.startsWith("data/")) {
    const base = f.file.slice(5).replace(/\.json$/i, "");
    const bm = base.match(/^Map\d+/i);
    if (bm) return bm[0];
    return "Database";
  }
  return f.file || "—";
}

/**
 * Groups visible findings by the active axis. Severity order is fixed
 * (error→warning→info), category order is canonical, map order is first appearance.
 */
export function groupFindings(items: IFinding[], by: GroupBy): Group[] {
  if (by === "category") {
    return CATEGORIES.map((cat) => ({
      groupBy: by,
      cat,
      name: cat,
      items: items.filter((x) => x.f.category === cat),
    })).filter((g) => g.items.length > 0);
  }
  if (by === "map") {
    const order: string[] = [];
    const buckets = new Map<string, IFinding[]>();
    for (const x of items) {
      const key = mapGroupKey(x.f);
      if (!buckets.has(key)) {
        buckets.set(key, []);
        order.push(key);
      }
      buckets.get(key)!.push(x);
    }
    return order.map((name) => ({ groupBy: by, name, items: buckets.get(name)! }));
  }
  return SEV_ORDER.map((sev) => ({
    groupBy: by,
    sev,
    name: sev,
    items: items.filter((x) => x.f.severity === sev),
  })).filter((g) => g.items.length > 0);
}

// ===========================================================================
// Atlas — joining findings to maps/events by their location path
// ===========================================================================

/** Map + event ids recovered from a finding's location (null when absent). */
export interface LocRef {
  map: number | null;
  event: number | null;
}

/**
 * Recovers (map, event) ids from a finding. The JSON flattens the location into
 * a breadcrumb string like `Map003/EV005/page2/cmd14`; we parse the `Map`/`EV`
 * segments. Map id also falls back to the `data/MapNNN.json` file name when the
 * path has no `Map` segment (some map-about findings anchor on System.json etc.).
 */
export function parseLoc(f: Finding): LocRef {
  let map: number | null = null;
  let event: number | null = null;
  for (const seg of f.path.split("/")) {
    const mm = seg.match(/^Map(\d+)/i);
    if (mm) {
      map = parseInt(mm[1], 10);
      continue;
    }
    const em = seg.match(/^EV(\d+)/i);
    if (em) event = parseInt(em[1], 10);
  }
  if (map === null) {
    const fm = f.file.match(/Map(\d+)\.json$/i);
    if (fm) map = parseInt(fm[1], 10);
  }
  return { map, event };
}

/** Full command location of a finding (map/event/page/cmd), or null. */
export interface CommandLoc {
  map: number;
  event: number;
  page: number;
  cmd: number;
}

/** Recovers map/event/page/command index from a finding, or null if incomplete. */
export function parseCommandLoc(f: Finding): CommandLoc | null {
  let map = -1;
  let event = -1;
  let page = -1;
  let cmd = -1;
  for (const seg of f.path.split("/")) {
    let m: RegExpMatchArray | null;
    if ((m = seg.match(/^Map(\d+)/i))) map = parseInt(m[1], 10);
    else if ((m = seg.match(/^EV(\d+)/i))) event = parseInt(m[1], 10);
    else if ((m = seg.match(/^page(\d+)/i))) page = parseInt(m[1], 10);
    else if ((m = seg.match(/^cmd(\d+)/i))) cmd = parseInt(m[1], 10);
  }
  if (map >= 0 && event >= 0 && page >= 0 && cmd >= 0) return { map, event, page, cmd };
  return null;
}

/** Index joining findings to maps and events for the Atlas view. */
export interface AtlasIndex {
  /** mapId → all finding indices on that map. */
  byMap: Map<number, number[]>;
  /** mapId → eventId → finding indices on that event. */
  byMapEvent: Map<number, Map<number, number[]>>;
  /** mapId → finding indices tied to the map but no specific event. */
  mapLevel: Map<number, number[]>;
  /** Finding indices with no map at all (switches/vars/plugins/db/assets). */
  project: number[];
}

function pushTo(m: Map<number, number[]>, key: number, i: number): void {
  const arr = m.get(key);
  if (arr) arr.push(i);
  else m.set(key, [i]);
}

/** Builds the {@link AtlasIndex} over non-ignored findings. */
export function buildAtlasIndex(report: Report, ignored: Set<number>): AtlasIndex {
  const byMap = new Map<number, number[]>();
  const byMapEvent = new Map<number, Map<number, number[]>>();
  const mapLevel = new Map<number, number[]>();
  const project: number[] = [];
  report.findings.forEach((f, i) => {
    if (ignored.has(i)) return;
    const { map, event } = parseLoc(f);
    if (map === null) {
      project.push(i);
      return;
    }
    pushTo(byMap, map, i);
    if (event === null) {
      pushTo(mapLevel, map, i);
      return;
    }
    let evMap = byMapEvent.get(map);
    if (!evMap) {
      evMap = new Map();
      byMapEvent.set(map, evMap);
    }
    pushTo(evMap, event, i);
  });
  return { byMap, byMapEvent, mapLevel, project };
}

/** Whether any finding on the given event is new since the previous run. */
export function eventIsNew(
  idx: AtlasIndex,
  map: number,
  event: number,
  diff: RunDiff,
): boolean {
  const arr = idx.byMapEvent.get(map)?.get(event);
  return !!arr && arr.some((i) => diff.newIdx.has(i));
}

/** Whether any finding on the given map (event-level or map-level) is new. */
export function mapHasNew(idx: AtlasIndex, map: number, diff: RunDiff): boolean {
  const arr = idx.byMap.get(map);
  return !!arr && arr.some((i) => diff.newIdx.has(i));
}

/** Event ids on a map carrying at least one finding new since the previous run. */
export function newEventsOnMap(idx: AtlasIndex, map: number, diff: RunDiff): Set<number> {
  const out = new Set<number>();
  const evMap = idx.byMapEvent.get(map);
  if (!evMap) return out;
  for (const [event, arr] of evMap) {
    if (arr.some((i) => diff.newIdx.has(i))) out.add(event);
  }
  return out;
}

/** Worst severity among the given finding indices (null if empty). */
export function worstSeverity(indices: number[], report: Report): Severity | null {
  let worst: Severity | null = null;
  for (const i of indices) {
    const s = report.findings[i].severity;
    if (s === "error") return "error";
    if (s === "warning") worst = "warning";
    else if (s === "info" && worst === null) worst = "info";
  }
  return worst;
}

/** Clusters a group's findings by rule (first-appearance order). */
export function clusterByRule(items: IFinding[]): Block[] {
  const order: string[] = [];
  const buckets = new Map<string, IFinding[]>();
  for (const x of items) {
    if (!buckets.has(x.f.rule)) {
      buckets.set(x.f.rule, []);
      order.push(x.f.rule);
    }
    buckets.get(x.f.rule)!.push(x);
  }
  return order.map((rule) => ({ rule, items: buckets.get(rule)! }));
}

/** All rule-ids actually present in the report. */
export function rulesInReport(r: Report): string[] {
  return [...new Set(r.findings.map((f) => f.rule))].sort();
}

// ===========================================================================
// Run diff — comparing a run against the previous one
// ===========================================================================

/**
 * Language-neutral fingerprint identifying a finding across runs. Built from the
 * rule id, file, breadcrumb path and the engine's structured args — NOT the
 * localized `message` — so switching the report language does not make every
 * finding look "new". The engine output is deterministic, so the same underlying
 * issue yields the same fingerprint on every run.
 */
export function fingerprint(f: Finding): string {
  return `${f.rule}${f.file}${f.path}${JSON.stringify(identityArgs(f.args))}`;
}

/**
 * Drops volatile per-finding counts (dead_variable.writes /
 * uninitialized_symbol.reads) from the args used for the fingerprint identity, so
 * an unrelated added/removed site does not make an unchanged finding look "new".
 * Mirrors the Rust `Finding::fingerprint` normalization.
 */
function identityArgs(args: Finding["args"]): unknown {
  if (args.key === "dead_variable") return { ...args, writes: 0 };
  if (args.key === "uninitialized_symbol") return { ...args, reads: 0 };
  return args;
}

/** Fingerprints of every finding in a report (index-aligned with `findings`). */
export function fingerprintsOf(report: Report): string[] {
  return report.findings.map(fingerprint);
}

/** Result of diffing a report against the previous run's fingerprints. */
export interface RunDiff {
  /** First run for this project — no comparison is shown. */
  baseline: boolean;
  /** Timestamp of the previous run compared against. */
  prevTs?: number;
  /** Indices (into `report.findings`) new since the previous run. */
  newIdx: Set<number>;
  /** Count of findings new since the previous run. */
  newCount: number;
  /** Count of findings present last run but gone now. */
  fixedCount: number;
}

/**
 * Diffs a fresh report against the previous run's fingerprints: which findings
 * are new, and how many were resolved. Pass `prevFps = null` for the first run.
 */
export function diffReport(prevFps: string[] | null, report: Report): RunDiff {
  if (!prevFps) {
    return { baseline: true, newIdx: new Set(), newCount: 0, fixedCount: 0 };
  }
  const prev = new Set(prevFps);
  const cur = new Set<string>();
  const newIdx = new Set<number>();
  report.findings.forEach((f, i) => {
    const fp = fingerprint(f);
    cur.add(fp);
    if (!prev.has(fp)) newIdx.add(i);
  });
  let fixedCount = 0;
  for (const fp of prev) if (!cur.has(fp)) fixedCount += 1;
  return { baseline: false, newIdx, newCount: newIdx.size, fixedCount };
}
