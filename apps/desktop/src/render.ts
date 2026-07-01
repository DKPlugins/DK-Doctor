import type {
  Category,
  CommandLine,
  Confidence,
  Finding,
  Lang,
  MapAtlas,
  MapGraph,
  ProjectStats,
  Report,
  Severity,
  UpdateInfo,
} from "./api";
import { computeHealth } from "./health";
import { computeReadiness, type GateStatus, type Verdict } from "./readiness";
import { scoreSparklineSVG } from "./timeline";
import { graphIsRenderable, graphStats, renderMapGraph } from "./mapgraph";
import type { RunHistoryPoint } from "./store";
import {
  AGG_THRESHOLD,
  CATEGORIES,
  CONFIDENCES,
  SEVERITIES,
  type AtlasIndex,
  type Filters,
  type GroupBy,
  type IFinding,
  type RunDiff,
  buildAtlasIndex,
  clusterByRule,
  computeTotals,
  groupFindings,
  mapHasNew,
  parseLoc,
  visible,
  worstSeverity,
} from "./group";
import { icon } from "./icons";
import type { RecentProject, Settings } from "./store";
import { cmdName, relTime, sevLabel, t } from "./i18n";

/** Application view. */
export type AppView = "welcome" | "scanning" | "report" | "error";

/** Full state visible to the renderer (single source of truth in main.ts). */
export interface State {
  view: AppView;
  settings: Settings;
  /** Effective language (system → OS locale). */
  lang: Lang;
  /** Effective theme (`light`/`dark`). */
  theme: "light" | "dark";
  project?: { path: string; name: string };
  report?: Report;
  stats?: ProjectStats;
  scannedAt?: number;
  error?: string;
  filters: Filters;
  groupBy: GroupBy;
  /** Expanded pattern-aggregation keys. */
  expanded: Set<string>;
  /** Hidden findings (indices into report.findings). */
  ignored: Set<number>;
  /** Index of the finding open in the drawer, or null. */
  drawer: number | null;
  recent: RecentProject[];
  /** Available update (if the check found a new version). */
  update?: UpdateInfo;
  /** Project files that could not be parsed (shown above the report). */
  warnings?: string[];
  /** Diff of this run against the previous run of the same project. */
  diff?: RunDiff;
  /** Whether the list is filtered to findings new since the last run. */
  newOnly: boolean;
  /** Report sub-view: spatial atlas (default), the flat list, or the map graph. */
  reportMode: "atlas" | "list" | "graph";
  /** Per-map geometry sidecar (undefined until loaded; failure keeps it unset). */
  atlas?: MapAtlas[];
  /** Map-transition graph sidecar (undefined until loaded; failure keeps it unset). */
  graph?: MapGraph;
  /** Open overlay modal (release readiness / time machine / export), or null. */
  overlay: null | "readiness" | "timeline" | "export";
  /** Run history of the open project for the Time Machine (loaded on scan). */
  history?: RunHistoryPoint[];
  /** Selected atlas target: a map id, the project board, the overview, or null. */
  atlasSel: number | "project" | "overview" | null;
  /** Selected event id on the current atlas map (null = none). */
  atlasEvent: number | null;
  /** Atlas map list filtered to maps with findings new since the last run. */
  atlasNewOnly: boolean;
  /** Region heat overlay enabled on the atlas canvas. */
  atlasRegions: boolean;
}

/** Index set the list is restricted to (new-only diff filter), or null. */
export function newRestrict(s: State): Set<number> | null {
  return s.newOnly && s.diff && !s.diff.baseline ? s.diff.newIdx : null;
}

/** Whether a meaningful run-diff exists to surface (not the first run). */
function hasDiff(s: State): boolean {
  return !!s.diff && !s.diff.baseline;
}

const SEV_ICON: Record<Severity, string> = {
  error: "octagon-alert",
  warning: "triangle-alert",
  info: "info",
};
const SEV_VAR: Record<Severity, string> = {
  error: "--sev-error",
  warning: "--sev-warning",
  info: "--sev-info",
};
const SEV_CLS: Record<Severity, "e" | "w" | "n"> = {
  error: "e",
  warning: "w",
  info: "n",
};
const CAT_ICON: Record<Category, string> = {
  data: "database",
  reference: "link",
  asset: "image",
  "dead-code": "code",
  "plugin-order": "list-ordered",
  "plugin-conflict": "puzzle",
};
const EMDASH = "—";

/** Escapes text for safe insertion as HTML. */
function esc(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

/**
 * Styles a finding message: the part after the first « — » (cause → symptom)
 * becomes secondary. The text is escaped (the engine returns plain text).
 */
function styleMessage(msg: string): string {
  const i = msg.indexOf(` ${EMDASH} `);
  if (i < 0) return esc(msg);
  const head = msg.slice(0, i);
  const tail = msg.slice(i + 1); // includes « — …»
  return `${esc(head)} <span class="em">${esc(tail)}</span>`;
}

/** Breadcrumb: file › seg / seg / leaf. */
function crumbHTML(file: string, path: string): string {
  const segs = path.split("/").filter(Boolean);
  let s = '<span class="crumb">';
  if (file) {
    s += `<span class="file">${esc(file)}</span>`;
    if (segs.length) s += '<span class="sep">›</span>';
  }
  segs.forEach((seg, i) => {
    const leaf = i === segs.length - 1;
    s += `<span class="${leaf ? "leaf" : ""}">${esc(seg)}</span>`;
    if (!leaf) s += '<span class="sep">/</span>';
  });
  return s + "</span>";
}

/** Confidence tag for a finding row (certain — no label). */
function confTag(c: Confidence): string {
  if (c === "likely") return '<span class="conf conf--likely">(likely)</span>';
  return "";
}

/** Group icon for the «Map» axis. */
function groupIcon(name: string): string {
  if (/^Map\d+/i.test(name)) return "map";
  if (name === "Plugins") return "puzzle";
  if (name === "Assets") return "image";
  if (name === "Database") return "database";
  return "file";
}

// ===========================================================================
// Appbar
// ===========================================================================
export function appbarHTML(s: State): string {
  const wm =
    '<span class="appbar__wm"><span class="pr">▸</span><span class="dk">dk</span>-doctor</span>';
  let proj = "";
  if (s.project && s.view !== "welcome") {
    proj =
      '<span class="appbar__sep"></span><span class="appbar__proj">' +
      `<span class="nm">${esc(s.project.name)}</span>` +
      `<span class="pt">${esc(s.project.path)}</span></span>`;
  }
  const themeIcon = s.theme === "dark" ? "moon" : "sun";
  // An explicit, labelled way to open/switch the project — once a project is
  // loaded the welcome picker is gone, so this is the discoverable affordance.
  const openBtn =
    s.view !== "welcome"
      ? `<button class="btn btn--ghost btn--sm appbar__open" data-act="pick">${icon("folder-open")} ${esc(t(s.lang, "openProject"))}</button>`
      : "";
  const ctrls =
    `<button class="flatbtn" data-act="toggle-theme" title="${t(s.lang, "theme")}" aria-label="${t(s.lang, "theme")}">${icon(themeIcon)}</button>` +
    `<button class="flatbtn" data-act="open-settings" title="${t(s.lang, "settings")}" aria-label="${t(s.lang, "settings")}">${icon("settings")}</button>`;
  return `${wm}${proj}<span class="appbar__spacer"></span><span class="appbar__ctrls">${openBtn}${ctrls}</span>`;
}

/** Update banner (empty if there is no update). */
export function updateBarHTML(s: State): string {
  if (!s.update) return "";
  const lang = s.lang;
  return (
    `<span class="updatebar__ic">${icon("arrow-up")}</span>` +
    `<span class="updatebar__msg">${esc(t(lang, "updateAvailable"))} <b>v${esc(s.update.version)}</b></span>` +
    `<button class="btn btn--primary btn--sm" data-act="get-update">${icon("download")} ${esc(t(lang, "updateGet"))}</button>` +
    `<button class="updatebar__x" data-act="dismiss-update" aria-label="${esc(t(lang, "updateLater"))}">${icon("x")}</button>`
  );
}

// ===========================================================================
// Welcome
// ===========================================================================
function scoreClass(n: number): string {
  return n >= 75 ? "ok" : n >= 50 ? "warn" : "err";
}

function recentRowHTML(lang: Lang, r: RecentProject): string {
  return (
    `<div class="recent__item" role="button" tabindex="0" data-open="${esc(r.path)}">` +
    `<span class="recent__ic">${icon(r.engine === "mv" ? "gamepad-2" : "folder")}</span>` +
    '<span class="recent__main">' +
    `<span class="recent__nm">${esc(r.name)}</span>` +
    `<span class="recent__meta"><span class="eng">${r.engine.toUpperCase()}</span> · ${esc(r.path)}</span></span>` +
    `<span class="recent__score recent__score--${scoreClass(r.score)}">${r.score}</span>` +
    `<span class="recent__time">${esc(relTime(lang, r.ts))}</span>` +
    `<button class="recent__del" data-del="${esc(r.path)}" title="${t(lang, "close")}" aria-label="${t(lang, "close")}">${icon("trash-2")}</button>` +
    "</div>"
  );
}

export function welcomeHTML(s: State): string {
  const lang = s.lang;
  const recent =
    s.recent.length > 0
      ? s.recent.map((r) => recentRowHTML(lang, r)).join("")
      : `<div class="recent__empty">${esc(t(lang, "wNoRecent"))}</div>`;
  return (
    '<div class="welcome"><div class="welcome__left">' +
    `<div class="welcome__brand"><span class="welcome__mark">${icon("stethoscope")}</span>` +
    '<span class="welcome__wm"><span class="pr">▸</span><span class="dk">dk</span>-doctor</span></div>' +
    '<div class="welcome__copy">' +
    `<span class="welcome__eyebrow">${esc(t(lang, "wEyebrow"))}</span>` +
    `<h1>${esc(t(lang, "wTitle"))}</h1>` +
    `<p class="welcome__lead">${esc(t(lang, "wLead"))}</p></div>` +
    '<div class="welcome__drop" id="drop">' +
    `<span class="di">${icon("folder-open")}</span>` +
    `<span class="dt">${esc(t(lang, "wDrop"))}</span>` +
    `<span class="dor">${esc(t(lang, "wOr"))}</span>` +
    `<button class="btn btn--primary btn--md" data-act="pick">${icon("folder")} ${esc(t(lang, "wOpen"))}</button></div>` +
    `<span class="welcome__note">${icon("circle-check")} ${esc(t(lang, "wNote"))}</span>` +
    badgesHTML(lang) +
    "</div>" +
    '<div class="welcome__right"><div class="welcome__rh">' +
    `<span class="t">${esc(t(lang, "wRecent"))}</span>` +
    `<button class="btn btn--ghost btn--sm" data-act="pick">${icon("plus")} ${esc(t(lang, "wNew"))}</button></div>` +
    `<div class="recent">${recent}</div></div></div>`
  );
}

// ===========================================================================
// Scanning shell
// ===========================================================================
export function scanShellHTML(s: State): string {
  const lang = s.lang;
  const name = s.project ? esc(s.project.name) : "";
  const stage = (k: number, label: string) =>
    `<span class="scan__stage" data-stage="${k}"><span class="d"></span>${esc(label)}</span>`;
  const count = (id: string, label: string) =>
    `<div class="scan__count"><div class="n" id="${id}">0</div><div class="l">${esc(label)}</div></div>`;
  return (
    '<div class="scan">' +
    `<div class="scan__head"><span class="scan__spin"></span><div>` +
    `<div class="scan__title">${esc(t(lang, "scanTitle"))} <span class="nm">${name}</span></div>` +
    `<div class="scan__sub">${esc(t(lang, "scanSub"))}</div></div></div>` +
    '<div class="scan__stages">' +
    stage(0, t(lang, "stReading")) +
    stage(1, t(lang, "stBuilding")) +
    stage(2, t(lang, "stRunning")) +
    stage(3, t(lang, "stRendering")) +
    "</div>" +
    '<div class="scan__bar is-indeterminate" id="scanBar"><i></i></div>' +
    '<div class="scan__counts">' +
    count("cMaps", t(lang, "cMaps")) +
    count("cEvents", t(lang, "cEvents")) +
    count("cAssets", t(lang, "cAssets")) +
    count("cPlugins", t(lang, "cPlugins")) +
    "</div>" +
    '<div class="well"><div class="well__lines" id="scanLines"><div><span class="well__cursor"></span></div></div></div>' +
    "</div>"
  );
}

// ===========================================================================
// Error state
// ===========================================================================
export function errorHTML(s: State): string {
  const lang = s.lang;
  return (
    '<div class="errstate">' +
    `<span class="ic">${icon("octagon-alert")}</span>` +
    `<h2>${esc(t(lang, "errTitle"))}</h2>` +
    `<div class="errstate__msg">${esc(s.error ?? "")}</div>` +
    `<div class="errstate__sub">${esc(t(lang, "errSub"))}</div>` +
    `<button class="btn btn--primary btn--md" data-act="pick">${icon("folder")} ${esc(t(lang, "pickAnother"))}</button>` +
    "</div>"
  );
}

// ===========================================================================
// Report — health / tiles / findings
// ===========================================================================
function ringHTML(score: number, ring: "ok" | "warn" | "err", grade: string, lang: Lang): string {
  const v = ring === "ok" ? "--sev-ok" : ring === "warn" ? "--sev-warning" : "--sev-error";
  const deg = score * 3.6;
  return (
    `<div class="health__ring" style="width:120px;height:120px;background:conic-gradient(var(${v}) ${deg}deg, var(--border-strong) 0)">` +
    '<div class="inner" style="margin:12px;width:96px;height:96px"><div>' +
    `<div class="score">${score}</div>` +
    `<div class="label">${esc(t(lang, "healthBadge"))}</div>` +
    `<div class="grade" style="color:var(${v})">${grade}</div>` +
    "</div></div></div>"
  );
}

function healthHTML(s: State, sev: Record<Severity, number>, all: number): string {
  const lang = s.lang;
  const h = computeHealth({ errors: sev.error, warnings: sev.warning, infos: sev.info });
  const maps = s.stats?.maps ?? 0;
  const denom = all || 1;
  const bar =
    '<div class="health__bar">' +
    `<i class="e" style="width:${(sev.error / denom) * 100}%"></i>` +
    `<i class="w" style="width:${(sev.warning / denom) * 100}%"></i>` +
    `<i class="n" style="width:${(sev.info / denom) * 100}%"></i></div>`;
  const line =
    '<div class="health__line">' +
    `<span class="lbl">${esc(t(lang, "total"))}</span> ` +
    `<span class="e">${sev.error}</span> ${esc(t(lang, "errors"))}, ` +
    `<span class="w">${sev.warning}</span> ${esc(t(lang, "warnings"))}, ` +
    `<span class="n">${sev.info}</span> ${esc(t(lang, "info"))}</div>`;
  const sub = `${all} ${esc(t(lang, "hAcross"))} ${maps} ${esc(t(lang, "hMaps"))}`;
  return (
    '<div class="health">' +
    ringHTML(h.score, h.ring, h.grade, lang) +
    '<div class="health__sum">' +
    `<div class="health__title">${esc(t(lang, "healthTitle"))}<span class="sub">${sub}</span></div>` +
    bar +
    line +
    "</div></div>"
  );
}

function tilesHTML(
  s: State,
  cat: Record<Category, number>,
  catBreak: Record<Category, Record<Severity, number>>,
): string {
  let html = '<div class="tiles">';
  for (const c of CATEGORIES) {
    let segs = "";
    for (const sev of SEVERITIES) {
      const n = catBreak[c][sev];
      if (n)
        segs += `<span class="tile__seg"><span class="tile__dot" style="background:var(${SEV_VAR[sev]})"></span>${n}</span>`;
    }
    if (!segs)
      segs = `<span class="tile__seg" style="color:var(--sev-ok-text)">${icon("check")}clean</span>`;
    const active =
      s.filters.category.size === 1 && s.filters.category.has(c) ? " is-active" : "";
    html +=
      `<button class="tile${active}" data-tile="${c}">` +
      `<span class="tile__head"><span class="tile__ico">${icon(CAT_ICON[c])}</span><span class="tile__name">${c}</span></span>` +
      `<span class="tile__count">${cat[c] ?? 0}</span>` +
      `<span class="tile__break">${segs}</span></button>`;
  }
  return html + "</div>";
}

function findingRowHTML(s: State, x: IFinding, compact: boolean): string {
  const f = x.f;
  const active = s.drawer === x.i ? " is-active" : "";
  const crumb = crumbHTML(f.file, f.path);
  const newTag =
    hasDiff(s) && s.diff!.newIdx.has(x.i)
      ? `<span class="frow__new">${esc(t(s.lang, "badgeNew"))}</span>`
      : "";
  const loc = parseLoc(f);
  const jump =
    loc.map !== null
      ? `<span class="frow__jump" role="button" tabindex="-1" data-showmap="${loc.map}:${loc.event ?? ""}" ` +
        `title="${esc(t(s.lang, "showOnMap"))}" aria-label="${esc(t(s.lang, "showOnMap"))}">${icon("map")}</span>`
      : "";
  let body =
    `<span class="frow__top">${crumb}` +
    `<span class="frow__meta">${newTag}${jump}<span class="frow__rule">(${esc(f.rule)})</span>${confTag(f.confidence)}` +
    `<span class="frow__chev">${icon("chevron-right")}</span></span></span>`;
  if (!compact) body += `<span class="frow__msg">${styleMessage(f.message)}</span>`;
  return (
    `<button class="frow frow--${f.severity}${active}" data-find="${x.i}">` +
    `<span class="frow__icon">${icon(SEV_ICON[f.severity])}</span>` +
    `<span class="frow__main">${body}</span></button>`
  );
}

function rowsHTML(s: State, gkey: string, items: IFinding[]): string {
  let html = "";
  for (const block of clusterByRule(items)) {
    if (block.items.length >= AGG_THRESHOLD) {
      const key = `${gkey}::${block.rule}`;
      const rest = block.items.length - 1;
      html += findingRowHTML(s, block.items[0], false);
      if (s.expanded.has(key)) {
        for (const x of block.items.slice(1)) html += findingRowHTML(s, x, true);
        html +=
          `<button class="fagg" data-collapse="${esc(key)}">${icon("chevron-up")}` +
          `<span>${esc(t(s.lang, "aggCollapse"))} · <span class="n">${rest}</span> ${esc(t(s.lang, "aggMore"))} · ${esc(block.rule)}</span></button>`;
      } else {
        html +=
          `<button class="fagg" data-expand="${esc(key)}">${icon("chevrons-down")}` +
          `<span><span class="n">${rest}</span> ${esc(t(s.lang, "aggMore"))} · ${esc(block.rule)}</span></button>`;
      }
    } else {
      for (const x of block.items) html += findingRowHTML(s, x, false);
    }
  }
  return html;
}

function emptyHTML(s: State, kind: "filter" | "clean"): string {
  const lang = s.lang;
  const title = kind === "clean" ? t(lang, "cleanTitle") : t(lang, "emptyTitle");
  const sub = kind === "clean" ? t(lang, "cleanSub") : t(lang, "emptySub");
  return (
    '<div class="fempty">' +
    `<span class="ic">${icon("circle-check")}</span>` +
    `<span class="t">${esc(title)}</span>` +
    `<span class="s">${esc(sub)}</span></div>`
  );
}

function sectionLabel(s: State, g: ReturnType<typeof groupFindings>[number]): string {
  const count = g.items.length;
  if (g.groupBy === "severity" && g.sev) {
    const cls = SEV_CLS[g.sev];
    return (
      '<div class="fgroup__label">' +
      `<span class="ic" style="color:var(${SEV_VAR[g.sev]})">${icon(SEV_ICON[g.sev])}</span>` +
      `<span class="nm ${cls}">${esc(sevLabel(s.lang, g.sev))}</span>` +
      `<span class="ln"></span><span class="cnt">${count}</span></div>`
    );
  }
  if (g.groupBy === "category" && g.cat) {
    return (
      '<div class="fgroup__label">' +
      `<span class="ic">${icon(CAT_ICON[g.cat])}</span>` +
      `<span class="nm mono">${esc(g.cat)}</span>` +
      `<span class="ln"></span><span class="cnt">${count}</span></div>`
    );
  }
  return (
    '<div class="fgroup__label">' +
    `<span class="ic">${icon(groupIcon(g.name))}</span>` +
    `<span class="nm mono">${esc(g.name)}</span>` +
    `<span class="ln"></span><span class="cnt">${count}</span></div>`
  );
}

/** Note shown above the report when some project files could not be parsed. */
function warningsHTML(s: State): string {
  const warns = s.warnings;
  if (!warns || warns.length === 0) return "";
  const lang = s.lang;
  const files = warns.map((w) => `<li>${esc(w)}</li>`).join("");
  return (
    '<div class="scanwarn">' +
    `<span class="scanwarn__ic">${icon("triangle-alert")}</span>` +
    `<div class="scanwarn__body"><span class="scanwarn__t">${warns.length} ${esc(t(lang, "warnFiles"))}</span>` +
    `<ul class="scanwarn__list">${files}</ul></div></div>`
  );
}

/** Banner summarizing what changed since the previous run (or "" on first run). */
function diffBarHTML(s: State): string {
  if (!hasDiff(s)) return "";
  const lang = s.lang;
  const d = s.diff!;
  const when = d.prevTs ? relTime(lang, d.prevTs) : "";
  const changed = d.newCount > 0 || d.fixedCount > 0;
  let chips = "";
  if (d.newCount > 0)
    chips += `<span class="diffchip diffchip--new">+${d.newCount} ${esc(t(lang, "diffNew"))}</span>`;
  if (d.fixedCount > 0)
    chips += `<span class="diffchip diffchip--fixed">−${d.fixedCount} ${esc(t(lang, "diffResolved"))}</span>`;
  const head = changed ? t(lang, "diffSince") : t(lang, "diffNoChanges");
  const toggle =
    d.newCount > 0
      ? `<button class="diffbar__btn" data-act="toggle-new" aria-pressed="${s.newOnly ? "true" : "false"}">` +
        `${icon(s.newOnly ? "list" : "sparkles")} ${esc(t(lang, s.newOnly ? "diffShowAll" : "diffShowNew"))}</button>`
      : "";
  return (
    `<div class="diffbar${changed ? "" : " diffbar--quiet"}">` +
    `<span class="diffbar__ic">${icon("history")}</span>` +
    `<span class="diffbar__txt">${esc(head)}${when ? ` <span class="diffbar__when">· ${esc(when)}</span>` : ""}</span>` +
    `<span class="diffbar__chips">${chips}</span>` +
    `<span class="diffbar__spacer"></span>${toggle}</div>`
  );
}

/** Content of the main area (health + tiles + grouped findings). */
export function mainHTML(s: State): string {
  const report = s.report!;
  const totals = computeTotals(report, s.ignored);
  let html = warningsHTML(s);
  html += diffBarHTML(s);
  html += healthHTML(s, totals.sev, totals.all);
  html += tilesHTML(s, totals.cat, totals.catBreak);

  const items = visible(report, s.filters, s.ignored, newRestrict(s));
  if (items.length === 0) {
    html += emptyHTML(s, totals.all === 0 && !s.newOnly ? "clean" : "filter");
    return html;
  }
  const groups = groupFindings(items, s.groupBy);
  html += '<div class="findings">';
  for (const g of groups) {
    const gkey = `${g.groupBy}:${g.name}`;
    html += `<div class="fgroup">${sectionLabel(s, g)}${rowsHTML(s, gkey, g.items)}</div>`;
  }
  html += "</div>";
  return html;
}

function railHTML(s: State): string {
  const lang = s.lang;
  const report = s.report!;
  const totals = computeTotals(report, s.ignored);
  const num = (n: number) => n.toLocaleString(lang === "ru" ? "ru-RU" : "en-US");
  const statRow = (l: string, v: string, brand = false) =>
    `<div class="row${brand ? " brandrow" : ""}"><span>${esc(l)}</span><span class="v">${esc(v)}</span></div>`;

  const st = s.stats;
  const stats =
    '<div class="rail__stats">' +
    statRow(t(lang, "stEngine"), (st?.engine ?? report.engine).toUpperCase(), true) +
    statRow(t(lang, "stMaps"), num(st?.maps ?? 0)) +
    statRow(t(lang, "stEvents"), num(st?.events ?? 0)) +
    statRow(t(lang, "stAssets"), num(st?.assets ?? 0)) +
    statRow(t(lang, "stPlugins"), num(st?.plugins ?? 0)) +
    "</div>";

  const clr = (group: keyof Filters) =>
    (s.filters[group] as Set<string>).size
      ? `<button class="clr" data-clearfacet="${group}">${esc(t(lang, "clear"))}</button>`
      : "";
  const facetRow = (
    group: "severity" | "category" | "confidence",
    val: string,
    label: string,
    count: number,
    opts: { swatch?: string; mono?: boolean } = {},
  ) => {
    const pressed = (s.filters[group] as Set<string>).has(val as never);
    const name =
      '<span class="facet__name">' +
      (opts.swatch ? `<span class="facet__swatch ${opts.swatch}"></span>` : "") +
      (opts.mono ? `<span class="mono">${esc(label)}</span>` : esc(label)) +
      "</span>";
    const attrs = ` data-facet="${group}" data-val="${esc(val)}"`;
    return (
      `<button class="facet__row" aria-pressed="${pressed ? "true" : "false"}"${attrs}>` +
      `<span class="facet__box">${icon("check")}</span>${name}` +
      `<span class="facet__count">${count}</span></button>`
    );
  };

  const sevFacet =
    `<div class="facet"><div class="facet__head"><span class="t">${esc(t(lang, "fSeverity"))}</span>${clr("severity")}</div>` +
    facetRow("severity", "error", sevLabel(lang, "error"), totals.sev.error, { swatch: "sw-error" }) +
    facetRow("severity", "warning", sevLabel(lang, "warning"), totals.sev.warning, { swatch: "sw-warning" }) +
    facetRow("severity", "info", sevLabel(lang, "info"), totals.sev.info, { swatch: "sw-info" }) +
    "</div>";

  let catFacet = `<div class="facet"><div class="facet__head"><span class="t">${esc(t(lang, "fCategory"))}</span>${clr("category")}</div>`;
  for (const c of CATEGORIES) catFacet += facetRow("category", c, c, totals.cat[c] ?? 0, { mono: true });
  catFacet += "</div>";

  let confFacet = `<div class="facet"><div class="facet__head"><span class="t">${esc(t(lang, "fConfidence"))}</span>${clr("confidence")}</div>`;
  for (const c of CONFIDENCES) confFacet += facetRow("confidence", c, c, totals.conf[c], { mono: true });
  confFacet += "</div>";

  return stats + sevFacet + catFacet + confFacet;
}

function segBtn(s: State, val: GroupBy, label: string): string {
  const on = s.groupBy === val;
  return `<button data-seg="${val}" aria-pressed="${on ? "true" : "false"}">${esc(label)}</button>`;
}

/** Full report: toolbar + (atlas | list). */
export function reportHTML(s: State): string {
  const lang = s.lang;
  const last = s.scannedAt ? `${t(lang, "lastRun")} ${relTime(lang, s.scannedAt)}` : "";
  const modeSeg =
    '<div class="seg seg--mode">' +
    `<button data-mode="atlas" aria-pressed="${s.reportMode === "atlas" ? "true" : "false"}">${icon("map")} ${esc(t(lang, "modeAtlas"))}</button>` +
    `<button data-mode="graph" aria-pressed="${s.reportMode === "graph" ? "true" : "false"}">${icon("waypoints")} ${esc(t(lang, "modeGraph"))}</button>` +
    `<button data-mode="list" aria-pressed="${s.reportMode === "list" ? "true" : "false"}">${icon("list")} ${esc(t(lang, "modeList"))}</button>` +
    "</div>";
  const groupSeg =
    s.reportMode === "list"
      ? `<div class="toolbar__group toolbar__group--hideable"><span class="tlabel">${esc(t(lang, "groupBy"))}</span><div class="seg">` +
        segBtn(s, "severity", t(lang, "gSeverity")) +
        segBtn(s, "category", t(lang, "gCategory")) +
        segBtn(s, "map", t(lang, "gMap")) +
        "</div></div>"
      : "";
  const watchOn = s.settings.watch;
  const toolbar =
    '<div class="toolbar">' +
    `<div class="toolbar__proj"><span class="nm">${esc(s.project?.name ?? "")}</span>` +
    (last ? `<span class="last">${esc(last)}</span>` : "") +
    "</div>" +
    '<span class="toolbar__spacer"></span>' +
    modeSeg +
    groupSeg +
    `<button class="iconbtn${watchOn ? " is-on" : ""}" data-act="toggle-watch" title="${esc(t(lang, "watchTip"))}" aria-label="${esc(t(lang, "watchTip"))}" aria-pressed="${watchOn ? "true" : "false"}">${icon("activity")}</button>` +
    `<button class="iconbtn" data-act="open-timeline" title="${esc(t(lang, "timelineTitle"))}" aria-label="${esc(t(lang, "timelineTitle"))}">${icon("history")}</button>` +
    `<button class="btn btn--ghost btn--md" data-act="open-readiness">${icon("clipboard-check")} ${esc(t(lang, "readyTitle"))}</button>` +
    `<button class="iconbtn" data-act="rerun" title="${esc(t(lang, "rerun"))}" aria-label="${esc(t(lang, "rerun"))}">${icon("refresh-cw")}</button>` +
    `<button class="btn btn--secondary btn--md" data-act="open-export">${icon("download")} ${esc(t(lang, "export"))}</button>` +
    "</div>";
  const body =
    s.reportMode === "atlas"
      ? atlasHTML(s)
      : s.reportMode === "graph"
        ? graphHTML(s)
        : `<div class="rv__cols">` +
          `<aside class="rail" aria-label="${esc(t(lang, "fSeverity"))}">${railHTML(s)}</aside>` +
          `<main class="main" id="reportMain">${mainHTML(s)}</main></div>`;
  return `<div class="rv">${toolbar}${body}</div>`;
}

// ===========================================================================
// Atlas — spatial map explorer
// ===========================================================================

/** A finding row by index, reusing the list renderer (data-find → drawer). */
export function findingRowById(s: State, i: number): string {
  const f = s.report?.findings[i];
  if (!f) return "";
  return findingRowHTML(s, { i, f }, false);
}

/**
 * Compact finding row for the event panel: the message leads, and the location
 * breadcrumb is dropped (we are already inside the event) — only the page chip,
 * rule and confidence remain. Keeps `data-find` so it opens the drawer.
 */
export function eventFindingRowHTML(s: State, i: number): string {
  const f = s.report?.findings[i];
  if (!f) return "";
  const active = s.drawer === i ? " is-active" : "";
  const page = f.path.split("/").find((seg) => /^page\d+/i.test(seg)) ?? "";
  const pageChip = page ? `<span class="crumb"><span class="leaf">${esc(page)}</span></span>` : "";
  const top =
    '<span class="frow__top">' +
    pageChip +
    `<span class="frow__meta"><span class="frow__rule">(${esc(f.rule)})</span>${confTag(f.confidence)}` +
    `<span class="frow__chev">${icon("chevron-right")}</span></span></span>`;
  return (
    `<button class="frow frow--${f.severity}${active}" data-find="${i}">` +
    `<span class="frow__icon">${icon(SEV_ICON[f.severity])}</span>` +
    `<span class="frow__main">${top}<span class="frow__msg">${styleMessage(f.message)}</span></span></button>`
  );
}

/** One map (or the project board) summarised for the atlas left rail. */
export interface AtlasRow {
  id: number;
  name: string;
  count: number;
  worst: Severity | null;
  hasGeom: boolean;
  /** Parent map id (0 = root) for the tree; 0 when unknown (findings-only). */
  parentId: number;
}

const SEV_RANK: Record<Severity, number> = { error: 0, warning: 1, info: 2 };

/** Maps for the atlas list: union of geometry + findings, worst-first. */
export function atlasMapRows(s: State, idx: AtlasIndex): AtlasRow[] {
  const report = s.report!;
  const rows = new Map<number, AtlasRow>();
  const geomIds = new Set<number>();
  for (const m of s.atlas ?? []) {
    geomIds.add(m.mapId);
    rows.set(m.mapId, {
      id: m.mapId,
      name: m.name || `Map ${m.mapId}`,
      count: 0,
      worst: null,
      hasGeom: true,
      parentId: m.parentId,
    });
  }
  for (const [mapId, indices] of idx.byMap) {
    let row = rows.get(mapId);
    if (!row) {
      row = {
        id: mapId,
        name: `Map ${mapId}`,
        count: 0,
        worst: null,
        hasGeom: geomIds.has(mapId),
        parentId: 0,
      };
      rows.set(mapId, row);
    }
    row.count = indices.length;
    row.worst = worstSeverity(indices, report);
  }
  const rank = (w: Severity | null) => (w ? SEV_RANK[w] : 3);
  return [...rows.values()].sort(
    (a, b) => rank(a.worst) - rank(b.worst) || b.count - a.count || a.id - b.id,
  );
}

/** Resolves a default atlas selection (overview heatmap when any map exists). */
export function resolveAtlasSel(s: State): number | "project" | "overview" | null {
  if (!s.report) return null;
  const idx = buildAtlasIndex(s.report, s.ignored);
  const rows = atlasMapRows(s, idx);
  if (rows.length) return "overview";
  if (idx.project.length) return "project";
  return null;
}

/** Args for one left-rail row (nav entry or tree node). */
interface RowOpts {
  sel: string;
  active: boolean;
  ic: string;
  name: string;
  count: number;
  worst: Severity | null;
  /** Tree depth (undefined = a plain nav entry without caret/indent). */
  depth?: number;
  hasKids?: boolean;
  open?: boolean;
  treeKey?: string;
}

function atlasRowHTML(o: RowOpts): string {
  const dot = o.worst
    ? `<span class="atlas__dot" style="background:var(${SEV_VAR[o.worst]})"></span>`
    : '<span class="atlas__dot atlas__dot--ok"></span>';
  const badge =
    o.count > 0
      ? `<span class="atlas__cnt atlas__cnt--${SEV_CLS[o.worst ?? "info"]}">${o.count}</span>`
      : `<span class="atlas__okmark">${icon("check")}</span>`;
  const caret =
    o.depth === undefined
      ? ""
      : o.hasKids
        ? `<span class="atlas__caret${o.open ? " is-open" : ""}" data-treetoggle="${esc(o.treeKey ?? "")}">${icon("chevron-right")}</span>`
        : '<span class="atlas__caret atlas__caret--leaf"></span>';
  const pad = o.depth ? ` style="padding-left:${o.depth * 14}px"` : "";
  return (
    `<div class="atlas__rowwrap"${pad}>${caret}` +
    `<button class="atlas__row${o.active ? " is-active" : ""}" data-mapsel="${esc(o.sel)}">` +
    `<span class="atlas__rowic">${icon(o.ic)}</span>${dot}` +
    `<span class="atlas__nm">${esc(o.name)}</span>${badge}</button></div>`
  );
}

/** Renders the map list as a parentId tree (worst-first per sibling group). */
function atlasTreeHTML(
  s: State,
  rows: AtlasRow[],
  sel: number | "project" | "overview" | null,
): string {
  const byId = new Map(rows.map((r) => [r.id, r]));
  const children = new Map<number, AtlasRow[]>();
  const roots: AtlasRow[] = [];
  for (const r of rows) {
    const p = r.parentId;
    // Orphan parents (absent from the row set) and self/0 parents become roots.
    if (p && p !== r.id && byId.has(p)) {
      const arr = children.get(p);
      if (arr) arr.push(r);
      else children.set(p, [r]);
    } else {
      roots.push(r);
    }
  }
  const rank = (w: Severity | null) => (w ? SEV_RANK[w] : 3);
  const sib = (a: AtlasRow, b: AtlasRow) =>
    rank(a.worst) - rank(b.worst) || b.count - a.count || a.id - b.id;
  roots.sort(sib);
  const visited = new Set<number>();
  let out = "";
  const walk = (r: AtlasRow, depth: number): void => {
    if (visited.has(r.id)) return; // cycle guard
    visited.add(r.id);
    const kids = (children.get(r.id) ?? []).slice().sort(sib);
    const treeKey = `atlasClosed:${r.id}`;
    const open = !s.expanded.has(treeKey);
    out += atlasRowHTML({
      sel: "" + r.id,
      active: sel === r.id,
      ic: "map",
      name: r.name,
      count: r.count,
      worst: r.worst,
      depth,
      hasKids: kids.length > 0,
      open,
      treeKey,
    });
    if (kids.length && open) for (const k of kids) walk(k, depth + 1);
  };
  for (const r of roots) walk(r, 0);
  // Recover any node trapped in a root-less parentId cycle (corrupt MapInfos):
  // it was never reached from a root, so surface it as its own root.
  for (const r of rows) if (!visited.has(r.id)) walk(r, 0);
  return out;
}

/** Overview heatmap: a grid of map cells colored by worst severity. */
function heatmapHTML(s: State, rows: AtlasRow[]): string {
  const lang = s.lang;
  let html =
    '<div class="atlas__detail atlas__detail--over"><div class="atlas__head">' +
    `<span class="atlas__title">${esc(t(lang, "atlasOverview"))}</span>` +
    '<span class="atlas__hsp"></span>' +
    `<span class="atlas__pill atlas__pill--info">${rows.length} ${esc(t(lang, "atlasMaps"))}</span></div>`;
  html += `<div class="atlas__overhint">${icon("info")}<span>${esc(t(lang, "atlasOverviewHint"))}</span></div>`;
  if (rows.length === 0) {
    return html + `<div class="atlas__empty">${esc(t(lang, "atlasClean"))}</div></div>`;
  }
  html += '<div class="atlas__heat">';
  for (const r of rows) {
    const cls = r.worst ? SEV_CLS[r.worst] : "ok";
    const mark =
      r.count > 0
        ? `<span class="heatcell__n">${r.count}</span>`
        : `<span class="heatcell__ok">${icon("check")}</span>`;
    html +=
      `<button class="heatcell heatcell--${cls}" data-mapsel="${r.id}" title="${esc(r.name)}">` +
      `<span class="heatcell__nm">${esc(r.name)}</span>${mark}</button>`;
  }
  return html + "</div></div>";
}

/** Atlas: left map list + right detail (overview / canvas / project board). */
export function atlasHTML(s: State): string {
  const report = s.report!;
  const lang = s.lang;
  const idx = buildAtlasIndex(report, s.ignored);
  const sel = s.atlasSel;
  const rows = atlasMapRows(s, idx);
  const diffOn = hasDiff(s);
  const shown =
    s.atlasNewOnly && diffOn ? rows.filter((r) => mapHasNew(idx, r.id, s.diff!)) : rows;
  const total = rows.reduce((a, r) => a + r.count, 0);

  let list = '<div class="atlas__list">';
  list += atlasRowHTML({
    sel: "overview",
    active: sel === "overview",
    ic: "layout-grid",
    name: t(lang, "atlasOverview"),
    count: total,
    worst: rows[0]?.worst ?? null,
  });
  list += atlasRowHTML({
    sel: "project",
    active: sel === "project",
    ic: "folder",
    name: t(lang, "atlasProject"),
    count: idx.project.length,
    worst: worstSeverity(idx.project, report),
  });
  if (diffOn) {
    list +=
      `<button class="atlas__newtog${s.atlasNewOnly ? " is-on" : ""}" data-act="atlas-newonly">` +
      `${icon("sparkles")}<span>${esc(t(lang, "diffShowNew"))}</span></button>`;
  }
  list += `<div class="atlas__listhead">${esc(t(lang, "atlasMaps"))}<span class="n">${shown.length}</span></div>`;
  list += atlasTreeHTML(s, shown, sel);
  list += "</div>";

  let detail: string;
  if (sel === "overview") detail = heatmapHTML(s, shown);
  else if (sel === "project") detail = projectBoardHTML(s, idx);
  else if (typeof sel === "number") detail = mapDetailHTML(s, idx, sel);
  else detail = `<div class="atlas__detail"><div class="atlas__empty">${esc(t(lang, "atlasPick"))}</div></div>`;

  return `<div class="atlas">${list}${detail}</div>`;
}

function mapDetailHTML(s: State, idx: AtlasIndex, mapId: number): string {
  const report = s.report!;
  const lang = s.lang;
  const geom = (s.atlas ?? []).find((m) => m.mapId === mapId);
  const mapFindings = idx.byMap.get(mapId) ?? [];
  const worst = worstSeverity(mapFindings, report);
  const name = geom?.name || `Map ${mapId}`;
  const dims = geom ? `${geom.width}×${geom.height}` : "";
  const pill = worst
    ? `<span class="atlas__pill atlas__pill--${SEV_CLS[worst]}">${icon(SEV_ICON[worst])} ${mapFindings.length} ${esc(t(lang, "atlasProblems"))}</span>`
    : `<span class="atlas__pill atlas__pill--ok">${icon("circle-check")} ${esc(t(lang, "atlasClean"))}</span>`;
  const header =
    '<div class="atlas__head">' +
    `<span class="atlas__title">${esc(name)}</span>` +
    (dims ? `<span class="atlas__dims">${esc(dims)}</span>` : "") +
    `<span class="atlas__hsp"></span>${pill}</div>`;

  let stage: string;
  if (geom) {
    const tools =
      '<div class="atlas__tools">' +
      `<button class="iconbtn${s.atlasRegions ? " is-on" : ""}" id="atlasRegions" title="${esc(t(lang, "atlasRegions"))}" aria-label="${esc(t(lang, "atlasRegions"))}">${icon("layers")}</button>` +
      `<button class="iconbtn" id="atlasWorst" title="${esc(t(lang, "atlasGoWorst"))}" aria-label="${esc(t(lang, "atlasGoWorst"))}">${icon("target")}</button>` +
      `<button class="iconbtn" id="atlasExport" title="${esc(t(lang, "atlasExportPng"))}" aria-label="${esc(t(lang, "atlasExportPng"))}">${icon("download")}</button>` +
      "</div>";
    const zoom =
      '<div class="atlas__zoom">' +
      tools +
      `<button class="iconbtn" id="atlasZoomOut" aria-label="−">${icon("minus")}</button>` +
      `<button class="iconbtn" id="atlasFit" aria-label="fit">${icon("maximize")}</button>` +
      `<button class="iconbtn" id="atlasZoomIn" aria-label="+">${icon("plus")}</button>` +
      "</div>";
    stage =
      '<div class="atlas__stage">' +
      `<div class="atlas__canvaswrap"><canvas id="atlasCanvas" class="atlas__canvas"></canvas>${zoom}</div>` +
      `<aside class="atlas__side" id="atlasEventPanel">${eventPanelHTML(s, mapId, s.atlasEvent)}</aside>` +
      "</div>";
  } else {
    const rowsHtml =
      mapFindings.map((i) => findingRowById(s, i)).join("") ||
      `<div class="atlas__empty">${esc(t(lang, "atlasClean"))}</div>`;
    stage =
      `<div class="atlas__nogeo">${icon("triangle-alert")} ${esc(t(lang, "atlasNoGeom"))}</div>` +
      `<div class="atlas__flat">${rowsHtml}</div>`;
  }
  return `<div class="atlas__detail">${header}${stage}</div>`;
}

/** Right-side panel listing the selected event's findings (and map-level ones). */
export function eventPanelHTML(s: State, mapId: number, eventId: number | null): string {
  const report = s.report!;
  const lang = s.lang;
  const idx = buildAtlasIndex(report, s.ignored);
  let html = "";
  if (eventId === null) {
    html += `<div class="atlas__sidehint">${icon("mouse-pointer-click")}<span>${esc(t(lang, "atlasSelectEvent"))}</span></div>`;
  } else {
    const evIndices = idx.byMapEvent.get(mapId)?.get(eventId) ?? [];
    const ev = (s.atlas ?? []).find((m) => m.mapId === mapId)?.events.find((e) => e.id === eventId);
    const label = `EV${String(eventId).padStart(3, "0")}`;
    const evName = ev?.name ? ` · ${ev.name}` : "";
    const coord = ev ? `(${ev.x}, ${ev.y})` : "";
    html +=
      '<div class="atlas__sidehead">' +
      `<span class="atlas__evt">${esc(label)}${esc(evName)}</span>` +
      `<span class="atlas__evc">${esc(coord)}</span></div>`;
    html += evIndices.length
      ? evIndices.map((i) => eventFindingRowHTML(s, i)).join("")
      : `<div class="atlas__sideok">${icon("circle-check")}<span>${esc(t(lang, "atlasEventClean"))}</span></div>`;
  }
  const mapLevel = idx.mapLevel.get(mapId) ?? [];
  if (mapLevel.length) {
    html += `<div class="atlas__sidesec">${esc(t(lang, "atlasMapLevel"))}</div>`;
    html += mapLevel.map((i) => eventFindingRowHTML(s, i)).join("");
  }
  return html;
}

function projectBoardHTML(s: State, idx: AtlasIndex): string {
  const report = s.report!;
  const lang = s.lang;
  let html =
    '<div class="atlas__detail"><div class="atlas__head">' +
    `<span class="atlas__title">${esc(t(lang, "atlasProject"))}</span>` +
    '<span class="atlas__hsp"></span>' +
    `<span class="atlas__pill atlas__pill--info">${idx.project.length} ${esc(t(lang, "atlasProblems"))}</span></div>`;
  html += '<div class="atlas__board">';
  if (idx.project.length === 0) {
    html += `<div class="atlas__empty">${esc(t(lang, "atlasProjectClean"))}</div>`;
  } else {
    for (const c of CATEGORIES) {
      const items = idx.project.filter((i) => report.findings[i].category === c);
      if (!items.length) continue;
      html +=
        '<div class="atlas__lane"><div class="atlas__lanehd">' +
        `<span class="ic">${icon(CAT_ICON[c])}</span><span class="nm mono">${esc(c)}</span>` +
        `<span class="n">${items.length}</span></div>` +
        items.map((i) => findingRowById(s, i)).join("") +
        "</div>";
    }
  }
  html += "</div></div>";
  return html;
}

// ===========================================================================
// Map graph — transition graph view (D6)
// ===========================================================================

/** Legend chip for the graph header. */
function graphLegend(swatch: string, label: string): string {
  return `<span class="mapgraph__leg"><span class="sw ${swatch}"></span>${esc(label)}</span>`;
}

/** Map-graph mode: transition graph of the project's maps (or its states). */
function graphHTML(s: State): string {
  const lang = s.lang;
  const report = s.report!;
  if (!s.graph) {
    return (
      '<div class="mapgraph"><div class="mapgraph__empty">' +
      `<span class="spin"></span><span>${esc(t(lang, "graphLoading"))}</span></div></div>`
    );
  }
  const g = s.graph;
  const stats = graphStats(g);
  if (!g.nodes.length) {
    return (
      '<div class="mapgraph"><div class="mapgraph__empty">' +
      `${icon("waypoints")}<span>${esc(t(lang, "graphEmpty"))}</span></div></div>`
    );
  }

  const pill = (cls: string, n: number, label: string) =>
    `<span class="mapgraph__pill mapgraph__pill--${cls}">${n} ${esc(label)}</span>`;
  const header =
    '<div class="mapgraph__head">' +
    `<span class="atlas__title">${esc(t(lang, "modeGraph"))}</span>` +
    '<span class="atlas__hsp"></span>' +
    pill("info", stats.reachable, t(lang, "graphReachable")) +
    (stats.islands > 0 ? pill("warn", stats.islands, t(lang, "graphIslands")) : "") +
    (stats.broken > 0 ? pill("err", stats.broken, t(lang, "graphBroken")) : "") +
    "</div>";

  const legend =
    '<div class="mapgraph__legend">' +
    graphLegend("lg-start", t(lang, "graphStart")) +
    graphLegend("lg-island", t(lang, "graphIslands")) +
    graphLegend("lg-broken", t(lang, "graphBroken")) +
    `<span class="mapgraph__leghint">${esc(t(lang, "graphHint"))}</span>` +
    "</div>";

  let stage: string;
  if (!graphIsRenderable(g)) {
    stage =
      `<div class="mapgraph__dense">${icon("triangle-alert")}` +
      `<span>${stats.nodes} ${esc(t(lang, "graphTooDense"))}</span></div>`;
  } else {
    const idx = buildAtlasIndex(report, s.ignored);
    const worst = new Map<number, Severity>();
    for (const [mapId, indices] of idx.byMap) {
      const w = worstSeverity(indices, report);
      if (w) worst.set(mapId, w);
    }
    const selected = typeof s.atlasSel === "number" ? s.atlasSel : null;
    const svg = renderMapGraph(g, {
      selected,
      worst,
      islandTitle: t(lang, "graphIsland"),
      brokenTitle: t(lang, "graphBrokenTip"),
    });
    stage = `<div class="mapgraph__scroll">${svg}</div>`;
  }

  return `<div class="mapgraph">${header}${legend}${stage}</div>`;
}

// ===========================================================================
// Command context (drawer)
// ===========================================================================

/**
 * Renders the event page's command list as a context block, highlighting the
 * line `cmd` (the command the finding points at). Indent reflects nesting.
 */
export function commandContextHTML(lang: Lang, lines: CommandLine[], cmd: number): string {
  if (!lines.length) return "";
  let rows = "";
  for (const c of lines) {
    const name = cmdName(lang, c.code);
    const hit = c.index === cmd;
    const pad = Math.max(0, c.indent) * 14;
    rows +=
      `<div class="dctx__row${hit ? " is-hit" : ""}"${hit ? ' data-hit="1"' : ""}>` +
      `<span class="dctx__n">${c.index}</span>` +
      `<span class="dctx__code" style="padding-left:${pad}px">${esc(name)}` +
      (c.arg ? ` <span class="dctx__arg">${esc(c.arg)}</span>` : "") +
      "</span></div>";
  }
  return (
    `<div class="dsec"><span class="dsec__t">${esc(t(lang, "dContext"))}</span>` +
    `<div class="dctx">${rows}</div></div>`
  );
}

// ===========================================================================
// Drawer
// ===========================================================================
function crumbInline(file: string, path: string): string {
  const segs = path.split("/").filter(Boolean);
  let s = "";
  if (file) {
    s += `<span class="file">${esc(file)}</span>`;
    if (segs.length) s += ' <span class="sep">›</span> ';
  }
  segs.forEach((seg, i) => {
    const leaf = i === segs.length - 1;
    s += `<span class="${leaf ? "leaf" : ""}">${esc(seg)}</span>`;
    if (!leaf) s += " / ";
  });
  return s;
}

function confLine(s: State, c: Confidence): string {
  const ic = c === "likely" ? "flask-conical" : "badge-check";
  const txt = c === "likely" ? t(s.lang, "cLikely") : t(s.lang, "cCertain");
  return `<div class="dconf">${icon(ic)}<span>${esc(txt)}</span></div>`;
}

/** Drawer content for the open finding (or "" if closed). */
export function drawerHTML(s: State): string {
  if (s.drawer === null || !s.report) return "";
  const f: Finding | undefined = s.report.findings[s.drawer];
  if (!f) return "";
  const lang = s.lang;
  const order = visible(s.report, s.filters, s.ignored, newRestrict(s)).map((x) => x.i);
  const idx = order.indexOf(s.drawer);
  const pos = idx >= 0 ? `${idx + 1} ${t(lang, "of")} ${order.length}` : "";
  const sevName = sevLabel(lang, f.severity);
  const newTag =
    hasDiff(s) && s.diff!.newIdx.has(s.drawer)
      ? `<span class="drawer__new">${esc(t(lang, "badgeNew"))}</span>`
      : "";

  const head =
    '<div class="drawer__bar">' +
    `<span class="drawer__sev drawer__sev--${f.severity}">${icon(SEV_ICON[f.severity])}${esc(sevName)}</span>` +
    `<span class="drawer__rule">(${esc(f.rule)})</span>${newTag}` +
    '<span class="drawer__nav">' +
    `<span class="pos">${esc(pos)}</span>` +
    `<button class="iconbtn" data-nav="prev" aria-label="${esc(t(lang, "navPrev"))}">${icon("chevron-up")}</button>` +
    `<button class="iconbtn" data-nav="next" aria-label="${esc(t(lang, "navNext"))}">${icon("chevron-down")}</button>` +
    `<button class="drawer__close" data-nav="close" aria-label="${esc(t(lang, "close"))}">${icon("x")}</button></span></div>`;

  let body = '<div class="drawer__body">';
  body += `<div class="dsec"><span class="dsec__t">${esc(t(lang, "dLocation"))}</span><div class="dcrumb">${crumbInline(f.file, f.path)}</div></div>`;
  body += `<div class="dsec"><span class="dsec__t">${esc(t(lang, "dDiagnosis"))}</span><div class="dmsg">${styleMessage(f.message)}</div></div>`;
  // Filled asynchronously with the event's command list (see loadDrawerContext).
  body += '<div id="drawerContext"></div>';
  body += `<div class="dsec">${confLine(s, f.confidence)}</div>`;
  if (f.references.length) {
    const refs = f.references
      .map(
        (r) =>
          `<div class="dref">${icon("corner-down-right")}<span class="c">${crumbInline(r.file, r.path)}</span></div>`,
      )
      .join("");
    body += `<div class="dsec"><span class="dsec__t">${esc(t(lang, "dRelated"))} · ${f.references.length}</span><div class="drefs">${refs}</div></div>`;
  }
  body += "</div>";

  const foot =
    '<div class="drawer__foot">' +
    `<button class="btn btn--secondary btn--md" data-act="copy">${icon("copy")} ${esc(t(lang, "copyPath"))}</button>` +
    '<span class="grow"></span>' +
    `<button class="iconbtn" data-act="ignore" title="${esc(t(lang, "ignore"))}" aria-label="${esc(t(lang, "ignore"))}">${icon("eye-off")}</button></div>`;

  return head + body + foot;
}

// ===========================================================================
// Settings
// ===========================================================================
export function settingsHTML(s: State): string {
  const lang = s.lang;
  const seg = (
    key: "theme" | "density" | "lang",
    val: string,
    label: string,
  ) => {
    const cur = String(s.settings[key]);
    return `<button data-set="${key}" data-val="${esc(val)}" aria-pressed="${cur === val ? "true" : "false"}">${esc(label)}</button>`;
  };
  const toggle = (
    key: "orphans" | "deadCommonEvents" | "checkUpdates",
    name: string,
    desc: string,
  ) =>
    `<button class="settings__toggle" data-toggle="${key}" aria-pressed="${s.settings[key] ? "true" : "false"}">` +
    `<span class="tx"><span class="nm">${esc(name)}</span><span class="ds">${esc(desc)}</span></span>` +
    '<span class="switch"></span></button>';

  return (
    '<div class="settings__bar">' +
    `<span class="t">${esc(t(lang, "setTitle"))}</span>` +
    `<button class="iconbtn settings__x x" data-act="close-settings" aria-label="${esc(t(lang, "close"))}">${icon("x")}</button></div>` +
    '<div class="settings__body">' +
    // theme
    `<div class="settings__field"><span class="settings__label">${esc(t(lang, "lblTheme"))}</span>` +
    '<div class="settings__seg">' +
    seg("theme", "system", t(lang, "thSystem")) +
    seg("theme", "light", t(lang, "thLight")) +
    seg("theme", "dark", t(lang, "thDark")) +
    "</div></div>" +
    // density
    `<div class="settings__field"><span class="settings__label">${esc(t(lang, "lblDensity"))}</span>` +
    '<div class="settings__seg">' +
    seg("density", "comfortable", t(lang, "dnComfortable")) +
    seg("density", "compact", t(lang, "dnCompact")) +
    "</div></div>" +
    // language
    `<div class="settings__field"><span class="settings__label">${esc(t(lang, "lblLanguage"))}</span>` +
    '<div class="settings__seg">' +
    seg("lang", "system", t(lang, "lnSystem")) +
    seg("lang", "ru", t(lang, "lnRu")) +
    seg("lang", "en", t(lang, "lnEn")) +
    "</div></div>" +
    // analysis
    `<div class="settings__field"><span class="settings__label">${esc(t(lang, "secAnalysis"))}</span>` +
    toggle("orphans", t(lang, "optOrphansName"), t(lang, "optOrphansDesc")) +
    toggle("deadCommonEvents", t(lang, "optDeadCEName"), t(lang, "optDeadCEDesc")) +
    "</div>" +
    // updates
    `<div class="settings__field"><span class="settings__label">${esc(t(lang, "secUpdates"))}</span>` +
    toggle("checkUpdates", t(lang, "optUpdatesName"), t(lang, "optUpdatesDesc")) +
    "</div>" +
    // data
    `<div class="settings__field"><span class="settings__label">${esc(t(lang, "secData"))}</span>` +
    `<button class="btn btn--secondary btn--md" data-act="clear-recent">${icon("trash-2")} ${esc(t(lang, "recentClear"))}</button></div>` +
    // about
    `<div class="settings__about"><span>${esc(t(lang, "aboutOffline"))}</span><span class="v">v0.1.0</span></div>` +
    "</div>"
  );
}

// ===========================================================================
// Trust badges (D8)
// ===========================================================================

/** Row of privacy/determinism badges (offline · deterministic · no telemetry). */
export function badgesHTML(lang: Lang): string {
  const b = (ic: string, label: string) =>
    `<span class="badge"><span class="badge__ic">${icon(ic)}</span>${esc(label)}</span>`;
  return (
    '<div class="badges">' +
    b("circle-check", t(lang, "badgeOffline")) +
    b("badge-check", t(lang, "badgeDeterministic")) +
    b("eye-off", t(lang, "badgeNoTelemetry")) +
    "</div>"
  );
}

// ===========================================================================
// Overlay modals — release readiness (D4), time machine (D7), export (D5/D10)
// ===========================================================================

const GATE_ICON: Record<GateStatus, string> = {
  pass: "circle-check",
  warn: "triangle-alert",
  fail: "octagon-alert",
};
const GATE_SEVVAR: Record<GateStatus, string> = {
  pass: "--sev-ok",
  warn: "--sev-warning",
  fail: "--sev-error",
};
const VERDICT_STATUS: Record<Verdict, GateStatus> = {
  ready: "pass",
  attention: "warn",
  blocked: "fail",
};

/** Shared overlay-card chrome (title bar + close + body). */
function overlayShell(lang: Lang, title: string, ic: string, body: string): string {
  return (
    '<div class="overlaycard">' +
    '<div class="overlaycard__bar">' +
    `<span class="t">${icon(ic)} ${esc(title)}</span>` +
    `<button class="iconbtn overlaycard__x" data-act="close-overlay" aria-label="${esc(t(lang, "close"))}">${icon("x")}</button>` +
    "</div>" +
    `<div class="overlaycard__body">${body}</div></div>`
  );
}

/** Release-readiness checklist body (D4). */
function readinessBody(s: State): string {
  const lang = s.lang;
  const r = computeReadiness(s.report!, s.ignored);
  const vs = VERDICT_STATUS[r.verdict];
  const banner =
    `<div class="ready__verdict ready__verdict--${vs}">` +
    `<span class="ic" style="color:var(${GATE_SEVVAR[vs]})">${icon(GATE_ICON[vs])}</span>` +
    `<div><div class="vt">${esc(t(lang, `readyVerdict_${r.verdict}`))}</div>` +
    `<div class="vs">${esc(t(lang, `readySub_${r.verdict}`))}</div></div></div>`;
  const gates = r.gates
    .map((g) => {
      const cnt =
        g.count > 0 ? `<span class="rgate__cnt">${g.count}</span>` : "";
      return (
        `<div class="rgate rgate--${g.status}">` +
        `<span class="rgate__ic" style="color:var(${GATE_SEVVAR[g.status]})">${icon(GATE_ICON[g.status])}</span>` +
        `<div class="rgate__main"><span class="rgate__nm">${esc(t(lang, `gate${g.key}Name`))}${cnt}</span>` +
        `<span class="rgate__ds">${esc(t(lang, `gate${g.key}Desc`))}</span></div></div>`
      );
    })
    .join("");
  return banner + `<div class="ready__gates">${gates}</div>`;
}

/** Time-Machine timeline body (D7). */
function timelineBody(s: State): string {
  const lang = s.lang;
  const hist = s.history ?? [];
  if (hist.length < 1) {
    return `<div class="tl__empty">${icon("history")}<span>${esc(t(lang, "timelineEmpty"))}</span></div>`;
  }
  const chart = `<div class="tl__chart">${scoreSparklineSVG(hist)}</div>`;
  // Newest first, with a delta against the chronologically-previous run.
  let rows = "";
  for (let i = hist.length - 1; i >= 0; i--) {
    const p = hist[i];
    const prev = i > 0 ? hist[i - 1] : null;
    const delta = prev ? p.score - prev.score : 0;
    const deltaHtml =
      prev && delta !== 0
        ? `<span class="tl__delta tl__delta--${delta > 0 ? "up" : "down"}">${delta > 0 ? "+" : ""}${delta}</span>`
        : '<span class="tl__delta tl__delta--flat">·</span>';
    const cls = scoreClass(p.score);
    rows +=
      '<div class="tl__row">' +
      `<span class="tl__when">${esc(relTime(lang, p.ts))}</span>` +
      `<span class="tl__score tl__score--${cls}">${p.score}</span>${deltaHtml}` +
      `<span class="tl__counts"><span class="e">${p.errors}</span> <span class="w">${p.warnings}</span> <span class="n">${p.infos}</span></span>` +
      "</div>";
  }
  return chart + `<div class="tl__runs">${rows}</div>`;
}

/** Export chooser body (D5/D10 + JSON). */
function exportBody(s: State): string {
  const lang = s.lang;
  const opt = (kind: string, ic: string, name: string, desc: string) =>
    `<button class="exopt" data-export="${kind}">` +
    `<span class="exopt__ic">${icon(ic)}</span>` +
    `<span class="exopt__tx"><span class="nm">${esc(name)}</span><span class="ds">${esc(desc)}</span></span>` +
    `<span class="exopt__chev">${icon("chevron-right")}</span></button>`;
  return (
    '<div class="exopts">' +
    opt("html", "file-text", t(lang, "exportHtmlName"), t(lang, "exportHtmlDesc")) +
    opt("png", "file-image", t(lang, "exportPngName"), t(lang, "exportPngDesc")) +
    opt("json", "braces", t(lang, "exportJsonName"), t(lang, "exportJsonDesc")) +
    "</div>"
  );
}

/** Renders the open overlay modal (or "" when none). */
export function overlayHTML(s: State): string {
  if (!s.overlay || !s.report) return "";
  const lang = s.lang;
  if (s.overlay === "readiness")
    return overlayShell(lang, t(lang, "readyTitle"), "clipboard-check", readinessBody(s));
  if (s.overlay === "timeline")
    return overlayShell(lang, t(lang, "timelineTitle"), "history", timelineBody(s));
  return overlayShell(lang, t(lang, "exportTitle"), "download", exportBody(s));
}
