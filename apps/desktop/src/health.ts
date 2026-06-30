import type { Summary } from "./api";

/** Health ring color: ok (green) / warn (yellow) / err (red). */
export type RingColor = "ok" | "warn" | "err";

/** Health computation result: score 0..100, grade letter, and ring color. */
export interface Health {
  score: number;
  grade: string;
  ring: RingColor;
}

const clamp = (n: number, lo: number, hi: number) =>
  Math.max(lo, Math.min(hi, n));

/**
 * Transparent project health formula:
 * `score = clamp(100 − (6·errors + 2·warnings + 0.5·infos), 0, 100)`.
 *
 * Ring: ≥75 ok, ≥50 warn, otherwise err. Grade: ≥90 A, ≥75 B, ≥60 C, ≥40 D, F.
 */
export function computeHealth(s: Summary): Health {
  const penalty = 6 * s.errors + 2 * s.warnings + 0.5 * s.infos;
  const score = clamp(Math.round(100 - penalty), 0, 100);
  const ring: RingColor = score >= 75 ? "ok" : score >= 50 ? "warn" : "err";
  const grade =
    score >= 90
      ? "A"
      : score >= 75
        ? "B"
        : score >= 60
          ? "C"
          : score >= 40
            ? "D"
            : "F";
  return { score, grade, ring };
}
