import type { Lang, ProjectStats, Report, Severity } from "./api";
import { computeHealth } from "./health";
import { computeReadiness, type GateStatus } from "./readiness";
import { sevLabel, t } from "./i18n";
import { CATEGORIES, computeTotals } from "./group";

/**
 * Standalone HTML report export (D5). Produces a single self-contained document
 * (inline CSS, no scripts, no external requests) that opens in any browser and
 * prints cleanly to PDF (a print stylesheet is embedded). The findings carry the
 * language already chosen for the run — nothing is re-analyzed here.
 */

/** Everything the HTML document needs (already computed on the app side). */
export interface HtmlReportInput {
  report: Report;
  stats?: ProjectStats;
  projectName: string;
  projectPath: string;
  lang: Lang;
  /** Generation timestamp (ms epoch). */
  generatedAt: number;
}

/** Minimal HTML escape for document text. */
function esc(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

const SEV_ORDER: Severity[] = ["error", "warning", "info"];
const SEV_COLOR: Record<Severity, string> = {
  error: "#c8383d",
  warning: "#ad7109",
  info: "#2f66b8",
};
const GATE_COLOR: Record<GateStatus, string> = {
  pass: "#2c834e",
  warn: "#ad7109",
  fail: "#c8383d",
};
const GATE_MARK: Record<GateStatus, string> = { pass: "✓", warn: "!", fail: "✕" };

/** Formats a timestamp for the report header in the given language. */
function fmtDate(ts: number, lang: Lang): string {
  return new Date(ts).toLocaleString(lang === "ru" ? "ru-RU" : "en-US", {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Renders a single finding row (location breadcrumb + message + rule). */
function findingHtml(
  loc: string,
  message: string,
  rule: string,
  confidence: string,
  color: string,
): string {
  const conf = confidence === "likely" ? ` <span class="conf">(likely)</span>` : "";
  return (
    `<div class="f" style="border-left-color:${color}">` +
    `<div class="floc">${esc(loc)}</div>` +
    `<div class="fmsg">${esc(message)}</div>` +
    `<div class="fmeta">(${esc(rule)})${conf}</div>` +
    "</div>"
  );
}

/**
 * Builds the full HTML document string for the report.
 */
export function buildReportHtml(input: HtmlReportInput): string {
  const { report, stats, projectName, projectPath, lang, generatedAt } = input;
  const ignored = new Set<number>();
  const totals = computeTotals(report, ignored);
  const health = computeHealth(report.summary);
  const readiness = computeReadiness(report, ignored);
  const ringColor =
    health.ring === "ok" ? "#2c834e" : health.ring === "warn" ? "#ad7109" : "#c8383d";
  const deg = health.score * 3.6;

  const verdictLabel = t(lang, `readyVerdict_${readiness.verdict}`);

  // Readiness gates.
  const gates = readiness.gates
    .map((g) => {
      const c = GATE_COLOR[g.status];
      const cnt = g.count > 0 ? ` <span class="gc">${g.count}</span>` : "";
      return (
        `<li><span class="gm" style="color:${c}">${GATE_MARK[g.status]}</span>` +
        `<span class="gn">${esc(t(lang, `gate${g.key}Name`))}${cnt}</span>` +
        `<span class="gd">${esc(t(lang, `gate${g.key}Desc`))}</span></li>`
      );
    })
    .join("");

  // Findings grouped by severity (only non-empty groups), each split by category
  // for a scannable structure.
  let findingsHtml = "";
  for (const sev of SEV_ORDER) {
    const inSev = report.findings.filter((f) => f.severity === sev);
    if (!inSev.length) continue;
    findingsHtml +=
      `<h2 class="sev" style="color:${SEV_COLOR[sev]}">${esc(sevLabel(lang, sev))}` +
      ` <span class="sevn">${inSev.length}</span></h2>`;
    for (const cat of CATEGORIES) {
      const rows = inSev.filter((f) => f.category === cat);
      if (!rows.length) continue;
      findingsHtml += `<h3 class="cat">${esc(cat)}</h3>`;
      for (const f of rows) {
        const loc = f.path ? `${f.file} › ${f.path}` : f.file;
        findingsHtml += findingHtml(loc, f.message, f.rule, f.confidence, SEV_COLOR[sev]);
      }
    }
  }
  if (!findingsHtml) {
    findingsHtml = `<div class="clean">${esc(t(lang, "cleanTitle"))}</div>`;
  }

  const statRow = (label: string, value: string) =>
    `<div class="srow"><span>${esc(label)}</span><b>${esc(value)}</b></div>`;
  const num = (n: number) => n.toLocaleString(lang === "ru" ? "ru-RU" : "en-US");
  const statsHtml = stats
    ? statRow(t(lang, "stEngine"), (stats.engine ?? report.engine).toUpperCase()) +
      statRow(t(lang, "stMaps"), num(stats.maps)) +
      statRow(t(lang, "stEvents"), num(stats.events)) +
      statRow(t(lang, "stAssets"), num(stats.assets)) +
      statRow(t(lang, "stPlugins"), num(stats.plugins))
    : "";

  const title = `dk-doctor — ${projectName}`;
  return `<!doctype html>
<html lang="${lang}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(title)}</title>
<style>
:root{color-scheme:light}
*{box-sizing:border-box}
body{margin:0;background:#f6f7f9;color:#181c22;font:15px/1.55 system-ui,-apple-system,Segoe UI,Roboto,sans-serif;padding:32px}
.wrap{max-width:900px;margin:0 auto}
.doc{background:#fff;border:1px solid #e1e5ea;border-radius:14px;padding:32px;box-shadow:0 1px 3px rgba(14,17,22,.06)}
.brand{font-weight:700;letter-spacing:-.02em;color:#0a807b;font-size:14px}
.brand .pr{color:#0a9d97}
h1{font-size:26px;margin:6px 0 2px}
.path{color:#5b6472;font-size:13px;word-break:break-all}
.gen{color:#8a929e;font-size:12px;margin-top:4px}
.top{display:flex;gap:28px;align-items:center;margin:24px 0;flex-wrap:wrap}
.ring{width:120px;height:120px;border-radius:50%;flex:0 0 auto;display:grid;place-items:center;
 background:conic-gradient(${ringColor} ${deg}deg,#e1e5ea 0)}
.ring .inner{width:96px;height:96px;border-radius:50%;background:#fff;display:grid;place-items:center;text-align:center}
.ring .score{font-size:32px;font-weight:700;line-height:1}
.ring .grade{color:${ringColor};font-weight:700}
.ring .lbl{font-size:11px;color:#8a929e;text-transform:uppercase;letter-spacing:.06em}
.sum{flex:1 1 260px;min-width:240px}
.verdict{font-size:18px;font-weight:700;margin-bottom:8px}
.counts{display:flex;gap:16px;font-size:14px;margin-bottom:10px}
.counts .e{color:#c8383d;font-weight:700}
.counts .w{color:#ad7109;font-weight:700}
.counts .n{color:#2f66b8;font-weight:700}
.stats{display:grid;grid-template-columns:1fr 1fr;gap:2px 24px;margin-top:6px}
.srow{display:flex;justify-content:space-between;border-bottom:1px solid #eef0f3;padding:3px 0;font-size:13px}
.srow span{color:#5b6472}
.gates{list-style:none;padding:0;margin:20px 0 4px}
.gates li{display:grid;grid-template-columns:22px 1fr;gap:2px 8px;padding:8px 0;border-bottom:1px solid #eef0f3}
.gates .gm{grid-row:span 2;font-weight:700;font-size:16px;text-align:center}
.gates .gn{font-weight:600}
.gates .gc{display:inline-block;background:#eef0f3;border-radius:10px;padding:0 7px;font-size:12px;color:#5b6472;font-weight:600}
.gates .gd{color:#5b6472;font-size:13px}
h2.sev{margin:26px 0 6px;font-size:16px;border-top:1px solid #e1e5ea;padding-top:18px}
h2.sev .sevn{background:#eef0f3;color:#5b6472;border-radius:10px;padding:0 8px;font-size:12px;vertical-align:middle}
h3.cat{margin:14px 0 6px;font-size:12px;text-transform:uppercase;letter-spacing:.05em;color:#8a929e;font-family:ui-monospace,monospace}
.f{border-left:3px solid #ccc;padding:6px 0 6px 12px;margin:6px 0}
.floc{font-size:12px;color:#5b6472;font-family:ui-monospace,monospace;word-break:break-all}
.fmsg{margin:2px 0}
.fmeta{font-size:12px;color:#8a929e;font-family:ui-monospace,monospace}
.fmeta .conf{color:#ad7109}
.clean{color:#2c834e;font-weight:600;padding:20px 0}
.foot{margin-top:28px;color:#8a929e;font-size:12px;text-align:center}
@media print{
 body{background:#fff;padding:0}
 .doc{border:0;box-shadow:none;border-radius:0}
 .f,.gates li{break-inside:avoid}
 h2.sev,h3.cat{break-after:avoid}
}
</style>
</head>
<body><div class="wrap"><div class="doc">
<div class="brand"><span class="pr">▸</span> dk-doctor</div>
<h1>${esc(projectName)}</h1>
<div class="path">${esc(projectPath)}</div>
<div class="gen">${esc(t(lang, "reportGenerated"))} ${esc(fmtDate(generatedAt, lang))}</div>
<div class="top">
 <div class="ring"><div class="inner"><div>
  <div class="score">${health.score}</div>
  <div class="grade">${esc(health.grade)}</div>
  <div class="lbl">${esc(t(lang, "healthBadge"))}</div>
 </div></div></div>
 <div class="sum">
  <div class="verdict" style="color:${GATE_COLOR[readiness.verdict === "ready" ? "pass" : readiness.verdict === "attention" ? "warn" : "fail"]}">${esc(verdictLabel)}</div>
  <div class="counts">
   <span><b class="e">${totals.sev.error}</b> ${esc(t(lang, "errors"))}</span>
   <span><b class="w">${totals.sev.warning}</b> ${esc(t(lang, "warnings"))}</span>
   <span><b class="n">${totals.sev.info}</b> ${esc(t(lang, "info"))}</span>
  </div>
  <div class="stats">${statsHtml}</div>
 </div>
</div>
<ul class="gates">${gates}</ul>
${findingsHtml}
<div class="foot">${esc(t(lang, "aboutOffline"))}</div>
</div></div></body></html>`;
}
