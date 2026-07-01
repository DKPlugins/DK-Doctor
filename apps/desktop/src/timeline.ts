import type { RunHistoryPoint } from "./store";

/**
 * Time Machine chart (D7): a compact score-over-time sparkline built as an SVG
 * from the per-project run history. Pure geometry — the surrounding modal and
 * run list live in render.ts.
 */

const W = 640;
const H = 170;
const PAD_L = 34;
const PAD_R = 12;
const PAD_T = 14;
const PAD_B = 22;

/** Score → ring color class token (matches the app severity palette). */
function scoreVar(score: number): string {
  return score >= 75 ? "--sev-ok" : score >= 50 ? "--sev-warning" : "--sev-error";
}

/**
 * Renders the score sparkline for a project's run history (oldest → newest).
 * Returns `""` for an empty history. A single run renders one dot.
 */
export function scoreSparklineSVG(points: RunHistoryPoint[]): string {
  if (!points.length) return "";
  const plotW = W - PAD_L - PAD_R;
  const plotH = H - PAD_T - PAD_B;
  const n = points.length;
  const x = (i: number) => PAD_L + (n === 1 ? plotW / 2 : (i / (n - 1)) * plotW);
  const y = (score: number) => {
    // A tampered localStorage history entry can carry a non-finite score, which
    // would otherwise yield NaN SVG coordinates and a broken sparkline.
    const s = Number.isFinite(score) ? score : 0;
    return PAD_T + (1 - Math.max(0, Math.min(100, s)) / 100) * plotH;
  };

  // Horizontal guides at 0 / 50 / 75 / 100 with left-axis labels.
  let grid = "";
  for (const g of [0, 50, 75, 100]) {
    const gy = y(g);
    grid +=
      `<line class="tl__grid" x1="${PAD_L}" y1="${gy}" x2="${W - PAD_R}" y2="${gy}"/>` +
      `<text class="tl__axis" x="${PAD_L - 6}" y="${gy + 3}" text-anchor="end">${g}</text>`;
  }

  // Score polyline + area under it.
  const pts = points.map((p, i) => `${x(i)},${y(p.score)}`).join(" ");
  const area =
    `M${x(0)},${y(0)} ` +
    points.map((p, i) => `L${x(i)},${y(p.score)}`).join(" ") +
    ` L${x(n - 1)},${y(0)} Z`;
  const line =
    n === 1
      ? ""
      : `<polyline class="tl__line" points="${pts}" fill="none"/>`;

  // Dots colored by that run's health band; the last one is emphasized.
  let dots = "";
  points.forEach((p, i) => {
    const last = i === n - 1;
    dots +=
      `<circle class="tl__dot${last ? " tl__dot--last" : ""}" cx="${x(i)}" cy="${y(p.score)}" ` +
      `r="${last ? 5 : 3.5}" style="fill:var(${scoreVar(p.score)})"/>`;
  });

  return (
    `<svg class="tl__svg" viewBox="0 0 ${W} ${H}" width="100%" preserveAspectRatio="xMidYMid meet" ` +
    `xmlns="http://www.w3.org/2000/svg">` +
    grid +
    `<path class="tl__area" d="${area}"/>` +
    line +
    dots +
    "</svg>"
  );
}
