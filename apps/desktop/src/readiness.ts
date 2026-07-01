import type { Category, Report, Severity } from "./api";
import { CATEGORIES } from "./group";

/**
 * Release-readiness checklist (D4): turns the findings report into a short,
 * honest set of ship gates. Nothing here re-runs the analyzer — it only
 * re-summarises the findings already produced, respecting the ignore set.
 */

/** A single gate's state: cleared, needs a look, or blocking. */
export type GateStatus = "pass" | "warn" | "fail";

/** One release gate (label/hint keys resolved via i18n on the render side). */
export interface Gate {
  /** i18n key stem: `gate<Key>Name` / `gate<Key>Desc`. */
  key: string;
  status: GateStatus;
  /** Findings backing this gate (0 when it is clean). */
  count: number;
}

/** Overall verdict: ready to ship, needs attention, or blocked. */
export type Verdict = "ready" | "attention" | "blocked";

/** Computed readiness for a report. */
export interface Readiness {
  gates: Gate[];
  verdict: Verdict;
  /** Total blocking errors and non-blocking warnings across the report. */
  errors: number;
  warnings: number;
}

/** Category → gate key stem (kept stable for i18n). */
const CAT_GATE: Record<Category, string> = {
  data: "Data",
  reference: "References",
  asset: "Assets",
  "dead-code": "DeadCode",
  "plugin-order": "Plugins",
  "plugin-conflict": "Plugins",
};

const rank: Record<Severity, number> = { error: 0, warning: 1, info: 2 };

/** Worse of two severities (lower rank wins). */
function worse(a: Severity | null, b: Severity): Severity {
  if (a === null) return b;
  return rank[b] < rank[a] ? b : a;
}

/** Maps a category's worst severity to a gate status (info does not block). */
function statusOf(worst: Severity | null): GateStatus {
  if (worst === "error") return "fail";
  if (worst === "warning") return "warn";
  return "pass";
}

/**
 * Computes the release-readiness checklist over the non-ignored findings.
 * Gates are grouped by concern (errors first, then per category family), each
 * carrying the count and worst severity of the findings behind it.
 */
export function computeReadiness(report: Report, ignored: Set<number>): Readiness {
  // Aggregate per gate key: worst severity + count.
  const worstBy = new Map<string, Severity | null>();
  const countBy = new Map<string, number>();
  let errors = 0;
  let warnings = 0;

  const gateKeys = new Set<string>();
  for (const c of CATEGORIES) gateKeys.add(CAT_GATE[c]);
  for (const k of gateKeys) {
    worstBy.set(k, null);
    countBy.set(k, 0);
  }

  report.findings.forEach((f, i) => {
    if (ignored.has(i)) return;
    if (f.severity === "error") errors += 1;
    else if (f.severity === "warning") warnings += 1;
    const key = CAT_GATE[f.category];
    if (key === undefined) return;
    worstBy.set(key, worse(worstBy.get(key) ?? null, f.severity));
    countBy.set(key, (countBy.get(key) ?? 0) + 1);
  });

  // The headline gate — no blocking errors — leads the list.
  const gates: Gate[] = [
    { key: "NoErrors", status: errors > 0 ? "fail" : "pass", count: errors },
  ];
  // Stable, product-ordered gate list (deduped, References/Assets/... families).
  for (const key of ["References", "Assets", "DeadCode", "Data", "Plugins"]) {
    if (!worstBy.has(key)) continue;
    gates.push({
      key,
      status: statusOf(worstBy.get(key) ?? null),
      count: countBy.get(key) ?? 0,
    });
  }

  const verdict: Verdict = errors > 0 ? "blocked" : warnings > 0 ? "attention" : "ready";
  return { gates, verdict, errors, warnings };
}
