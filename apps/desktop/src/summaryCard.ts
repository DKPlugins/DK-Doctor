import type { Lang, ProjectStats, Report, Severity } from "./api";
import { computeHealth } from "./health";
import { computeReadiness } from "./readiness";
import { sevLabel, t } from "./i18n";

/**
 * Shareable summary card (D10): renders the health headline of a run onto an
 * offscreen canvas and returns a PNG data URL. Pure drawing — no findings are
 * recomputed beyond the totals already in the report. Fixed 1200×630 (the common
 * social-card ratio) so it embeds well anywhere.
 */

export interface SummaryCardInput {
  report: Report;
  stats?: ProjectStats;
  projectName: string;
  lang: Lang;
  /** Generation timestamp (ms epoch). */
  generatedAt: number;
}

const W = 1200;
const H = 630;

const INK = "#181c22";
const MUTED = "#5b6472";
const FAINT = "#8a929e";
const BORDER = "#e1e5ea";
const BRAND = "#0a807b";
const SEV_COLOR: Record<Severity, string> = {
  error: "#c8383d",
  warning: "#ad7109",
  info: "#2f66b8",
};

/** Rounded-rectangle path helper. */
function roundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
): void {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.arcTo(x + w, y, x + w, y + h, r);
  ctx.arcTo(x + w, y + h, x, y + h, r);
  ctx.arcTo(x, y + h, x, y, r);
  ctx.arcTo(x, y, x + w, y, r);
  ctx.closePath();
}

/** Truncates text to fit `maxW` px in the current font, adding an ellipsis. */
function fit(ctx: CanvasRenderingContext2D, text: string, maxW: number): string {
  if (ctx.measureText(text).width <= maxW) return text;
  let s = text;
  while (s.length > 1 && ctx.measureText(s + "…").width > maxW) s = s.slice(0, -1);
  return s + "…";
}

const SANS = "system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif";
const MONO = "ui-monospace, 'SF Mono', Menlo, monospace";

/**
 * Draws the summary card and returns it as a PNG data URL. Returns `null` if a
 * 2D context is unavailable (headless/edge cases) — callers fall back to a toast.
 */
export function renderSummaryCard(input: SummaryCardInput): string | null {
  const { report, stats, projectName, lang, generatedAt } = input;
  const canvas = document.createElement("canvas");
  canvas.width = W;
  canvas.height = H;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;

  const health = computeHealth(report.summary);
  const readiness = computeReadiness(report, new Set());
  const su = report.summary;
  const ringColor =
    health.ring === "ok" ? "#2c834e" : health.ring === "warn" ? "#ad7109" : "#c8383d";

  // Backdrop + card.
  ctx.fillStyle = "#eef1f4";
  ctx.fillRect(0, 0, W, H);
  ctx.fillStyle = "#ffffff";
  roundRect(ctx, 24, 24, W - 48, H - 48, 28);
  ctx.fill();
  ctx.strokeStyle = BORDER;
  ctx.lineWidth = 2;
  ctx.stroke();

  const padX = 72;

  // Header: brand + date.
  ctx.textBaseline = "alphabetic";
  ctx.fillStyle = BRAND;
  ctx.font = `700 26px ${SANS}`;
  ctx.fillText("▸ dk-doctor", padX, 92);
  const date = new Date(generatedAt).toLocaleDateString(
    lang === "ru" ? "ru-RU" : "en-US",
    { year: "numeric", month: "short", day: "numeric" },
  );
  ctx.fillStyle = FAINT;
  ctx.font = `500 20px ${MONO}`;
  ctx.textAlign = "right";
  ctx.fillText(date, W - padX, 90);
  ctx.textAlign = "left";

  // Project name + engine.
  ctx.fillStyle = INK;
  ctx.font = `700 52px ${SANS}`;
  ctx.fillText(fit(ctx, projectName, W - padX * 2 - 120), padX, 168);
  if (stats) {
    ctx.fillStyle = MUTED;
    ctx.font = `600 22px ${MONO}`;
    ctx.fillText((stats.engine ?? report.engine).toUpperCase(), padX, 208);
  }

  // Health ring (left).
  const cx = padX + 100;
  const cy = 380;
  const rad = 92;
  ctx.lineWidth = 22;
  ctx.strokeStyle = BORDER;
  ctx.beginPath();
  ctx.arc(cx, cy, rad, 0, Math.PI * 2);
  ctx.stroke();
  ctx.strokeStyle = ringColor;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.arc(cx, cy, rad, -Math.PI / 2, -Math.PI / 2 + (health.score / 100) * Math.PI * 2);
  ctx.stroke();
  ctx.lineCap = "butt";
  ctx.fillStyle = INK;
  ctx.textAlign = "center";
  ctx.font = `700 62px ${SANS}`;
  ctx.fillText(String(health.score), cx, cy + 12);
  ctx.fillStyle = ringColor;
  ctx.font = `700 26px ${SANS}`;
  ctx.fillText(health.grade, cx, cy + 46);
  ctx.fillStyle = FAINT;
  ctx.font = `600 16px ${SANS}`;
  ctx.fillText(t(lang, "healthBadge").toUpperCase(), cx, cy - 40);
  ctx.textAlign = "left";

  // Verdict + counts (right of the ring).
  const rx = cx + rad + 56;
  const verdictColor =
    readiness.verdict === "ready" ? "#2c834e" : readiness.verdict === "attention" ? "#ad7109" : "#c8383d";
  ctx.fillStyle = verdictColor;
  ctx.font = `700 32px ${SANS}`;
  ctx.fillText(fit(ctx, t(lang, `readyVerdict_${readiness.verdict}`), W - rx - padX), rx, 312);

  const counts: [Severity, number][] = [
    ["error", su.errors],
    ["warning", su.warnings],
    ["info", su.infos],
  ];
  let bx = rx;
  for (const [sev, n] of counts) {
    ctx.fillStyle = SEV_COLOR[sev];
    ctx.font = `700 40px ${SANS}`;
    const nStr = String(n);
    ctx.fillText(nStr, bx, 372);
    const nw = ctx.measureText(nStr).width;
    ctx.fillStyle = MUTED;
    ctx.font = `500 20px ${SANS}`;
    const label = sevLabel(lang, sev);
    ctx.fillText(label, bx, 400);
    bx += Math.max(nw, ctx.measureText(label).width) + 40;
  }

  // Top risks (up to 3, errors first then warnings).
  const ranked = report.findings
    .map((f, i) => ({ f, i }))
    .filter((x) => x.f.severity !== "info")
    .sort((a, b) => (a.f.severity === b.f.severity ? 0 : a.f.severity === "error" ? -1 : 1))
    .slice(0, 3);
  let ty = 462;
  if (ranked.length) {
    ctx.fillStyle = FAINT;
    ctx.font = `600 16px ${SANS}`;
    ctx.fillText(t(lang, "cardTopRisks").toUpperCase(), rx, ty);
    ty += 30;
    for (const { f } of ranked) {
      ctx.fillStyle = SEV_COLOR[f.severity];
      ctx.beginPath();
      ctx.arc(rx + 6, ty - 6, 6, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = INK;
      ctx.font = `500 21px ${SANS}`;
      const head = f.message.split(" — ")[0];
      ctx.fillText(fit(ctx, head, W - rx - padX - 24), rx + 22, ty);
      ty += 34;
    }
  } else {
    ctx.fillStyle = "#2c834e";
    ctx.font = `600 22px ${SANS}`;
    ctx.fillText(t(lang, "cleanTitle"), rx, ty + 8);
  }

  return canvas.toDataURL("image/png");
}
