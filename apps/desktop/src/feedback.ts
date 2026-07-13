/**
 * Feedback payload builder — turns a report (or a single finding) into a compact,
 * sanitised Markdown block the user copies to the clipboard and pastes into a
 * GitHub issue. The app never uploads anything: this text is the *only* thing
 * that leaves the machine, and only by the user's explicit copy + paste.
 *
 * Privacy contract (surfaced verbatim in the share dialog): the payload carries
 * finding metadata (rule, severity, confidence, message, project-relative paths,
 * fingerprint) plus the engine and app version — never map/event contents,
 * images, audio, plugin source, or absolute filesystem paths.
 */
import type { Finding, Lang, Report } from "./api";

/** Fixed GitHub target the user pastes into. Plain https, no query — the
 *  backend `open_url` guard rejects `&` and long URLs, so we never prefill. */
export const FEEDBACK_ISSUE_URL = "https://github.com/DKPlugins/DK-Doctor/issues/new";

/** Project roots whose first appearance marks the start of a relative path. */
const ROOTS = ["data/", "img/", "audio/", "js/", "movies/", "fonts/", "effects/"];

/**
 * Absolute-path tokens, anywhere in the string: a drive-letter path (spaces
 * allowed — Windows paths often contain them — up to the next colon), a UNC
 * path, or a whitespace-delimited POSIX-root path. Over-matching is fine
 * (worst case a few trailing words get folded into the basename); the failure
 * mode to avoid is letting an absolute path through.
 */
const ABS_PATH = /[A-Za-z]:\/[^:<>|"\n]*|\/\/[^\s:]+|(^|\s)(\/[^\s:]+)/g;

/**
 * Cuts `path` down to its project-relative tail (earliest known root wins).
 * When `abs` is set and no root is found, keeps only the basename so nothing
 * absolute survives; relative strings without a root pass through untouched.
 */
function relativize(path: string, abs: boolean): string {
  const lower = path.toLowerCase();
  let cut = -1;
  for (const root of ROOTS) {
    if (lower.startsWith(root)) return path;
    const at = lower.indexOf("/" + root);
    if (at >= 0 && (cut < 0 || at + 1 < cut)) cut = at + 1;
  }
  if (cut >= 0) return path.slice(cut);
  if (!abs) return path;
  return path.slice(path.lastIndexOf("/") + 1).trim();
}

/**
 * Reduces a path to its project-relative tail so no absolute path or drive
 * letter leaves the machine. Report finding paths are already relative; this
 * mainly hardens load-warning strings, which may embed the full OS path
 * anywhere in the text (e.g. `config C:/Users/…/config.toml: TOML error`).
 * Deny-by-default: every absolute token is rewritten, known root or not.
 */
export function sanitizePath(p: string): string {
  const s = p.replace(/\\/g, "/").trim();
  const scrubbed = s.replace(ABS_PATH, (m, pre: string | undefined, posix: string | undefined) =>
    posix !== undefined ? `${pre}${relativize(posix, true)}` : relativize(m, true),
  );
  return relativize(scrubbed, false);
}

/** One finding as a Markdown list item (English field labels for universality). */
function findingBlock(f: Finding): string {
  const at = f.path ? `${sanitizePath(f.file)} › ${f.path}` : sanitizePath(f.file);
  const lines = [
    `- **[${f.severity} · ${f.confidence}]** \`${f.rule}\` — ${f.message}`,
    `  - at: ${at}`,
    `  - id: ${f.fingerprint}`,
  ];
  return lines.join("\n");
}

/** Options for {@link buildFeedback}. */
export interface FeedbackOpts {
  report: Report;
  version: string;
  lang: Lang;
  /** When set, only this finding index is included (a false-positive report). */
  finding?: number | null;
  /** Indices the user hid — excluded from the whole-report payload. */
  ignored?: Set<number>;
  /** Load warnings (skipped files); included sanitised for the full report. */
  warnings?: string[];
}

/** Human note appended so the maintainer knows exactly what was (not) shared. */
function footer(lang: Lang): string {
  return lang === "ru"
    ? "_Отправлено из DK-Doctor. Только метаданные находок (правило, сообщение, относительные пути). Без содержимого карт, изображений, аудио, исходников плагинов и абсолютных путей._"
    : "_Sent from DK-Doctor. Finding metadata only (rule, message, relative paths). No map contents, images, audio, plugin source, or absolute paths._";
}

/**
 * Builds the Markdown payload. A single `finding` produces a false-positive
 * report with a prompt for the reason; otherwise the whole report minus the
 * findings the user hid (`ignored`), with the summary counts matching.
 * Returns "" only when the requested finding is missing.
 */
export function buildFeedback(o: FeedbackOpts): string {
  const { report, version, lang } = o;
  const engine = report.engine.toUpperCase();

  if (o.finding != null) {
    const f = report.findings[o.finding];
    if (!f) return "";
    const title =
      lang === "ru"
        ? "### DK-Doctor — возможное ложное срабатывание"
        : "### DK-Doctor — possible false positive";
    const ask =
      lang === "ru"
        ? "> Почему это ложное срабатывание? (опишите здесь)"
        : "> Why is this a false positive? (describe here)";
    return [
      title,
      `app ${version} · engine ${engine}`,
      "",
      findingBlock(f),
      "",
      ask,
      "",
      footer(lang),
      "",
    ].join("\n");
  }

  const visible = report.findings.filter((_, i) => !o.ignored?.has(i));
  const count = (sev: string) => visible.filter((f) => f.severity === sev).length;
  const parts: string[] = [
    lang === "ru" ? "### Отчёт DK-Doctor" : "### DK-Doctor report",
    `app ${version} · engine ${engine} · ${count("error")} error / ${count("warning")} warning / ${count("info")} info`,
    "",
  ];
  const body = visible.map(findingBlock).join("\n");
  if (body) parts.push(body, "");

  const warns = (o.warnings ?? []).map(sanitizePath).filter(Boolean);
  if (warns.length) {
    const head =
      lang === "ru"
        ? `<details><summary>Пропущенные файлы (${warns.length})</summary>`
        : `<details><summary>Skipped files (${warns.length})</summary>`;
    parts.push(head, "", warns.slice(0, 50).map((w) => `- ${w}`).join("\n"), "", "</details>", "");
  }

  parts.push(footer(lang), "");
  return parts.join("\n");
}
