//! Console report renderer: findings grouped by severity level,
//! plus a summary banner with counters.
//!
//! All human-readable text (headings, tags, messages, summary, hint)
//! comes from the core catalog ([`render`]/[`render_chrome`]) in the selected language;
//! the renderer only colorizes the result. Color is applied via `owo-colors` on top of
//! `anstream`, which itself strips ANSI when output goes to a pipe/file — so
//! `--format json` and redirection stay clean, while the Windows terminal
//! receives correct VT sequences.

use anstream::println;
use dk_doctor_core::{Chrome, Confidence, Finding, Lang, Report, Severity, render, render_chrome};
use owo_colors::OwoColorize;

/// Prints the report in human-readable form in the selected language.
pub fn render_report(report: &Report, engine: &str, lang: Lang) {
    println!();
    println!(
        "{} {}",
        "dk-doctor".bold().cyan(),
        render_chrome(
            &Chrome::Header {
                engine: engine.to_string()
            },
            lang
        )
        .dimmed()
    );

    if report.findings.is_empty() {
        println!();
        println!(
            "{}",
            format!("  {}", render_chrome(&Chrome::NoProblems, lang)).green()
        );
        print_summary(report, lang);
        return;
    }

    let mut current: Option<Severity> = None;
    for f in &report.findings {
        if current != Some(f.severity) {
            current = Some(f.severity);
            println!();
            println!("{}", severity_heading(f.severity, lang));
        }
        print_finding(f, lang);
    }

    print_summary(report, lang);
}

/// Prints a hint that `orphan-assets` is disabled by default.
///
/// Not a "silent skip": the user sees that unused assets were not
/// checked, and how to enable them.
pub fn print_orphans_hint(lang: Lang) {
    println!(
        "  {} {}",
        render_chrome(&Chrome::HintPrefix, lang).dimmed(),
        render_chrome(&Chrome::OrphansHint, lang)
    );
    println!();
}

/// Prints a hint that `dead-common-event` is disabled by default.
///
/// Plugins often reserve common events via `$gameTemp.reserveCommonEvent`
/// (not tracked), so the rule is opt-in, like `orphan-assets`.
pub fn print_dead_common_events_hint(lang: Lang) {
    println!(
        "  {} {}",
        render_chrome(&Chrome::HintPrefix, lang).dimmed(),
        render_chrome(&Chrome::DeadCommonEventsHint, lang)
    );
    println!();
}

/// Prints the list of project files that could not be parsed (skipped), so the
/// report is not silently partial. A no-op when there are no warnings.
pub fn print_parse_warnings(warnings: &[String], lang: Lang) {
    if warnings.is_empty() {
        return;
    }
    println!();
    println!(
        "  {}",
        render_chrome(
            &Chrome::ParseWarningsHeader {
                count: warnings.len()
            },
            lang
        )
        .yellow()
    );
    for w in warnings {
        println!("    {}", w.dimmed());
    }
    println!();
}

/// Section heading by severity level.
fn severity_heading(sev: Severity, lang: Lang) -> String {
    match sev {
        Severity::Error => render_chrome(&Chrome::HeadingError, lang)
            .bold()
            .red()
            .to_string(),
        Severity::Warning => render_chrome(&Chrome::HeadingWarning, lang)
            .bold()
            .yellow()
            .to_string(),
        Severity::Info => render_chrome(&Chrome::HeadingInfo, lang)
            .bold()
            .blue()
            .to_string(),
    }
}

/// Colored severity tag for a finding line.
fn severity_tag(sev: Severity) -> String {
    match sev {
        Severity::Error => "[error]  ".red().to_string(),
        Severity::Warning => "[warning]".yellow().to_string(),
        Severity::Info => "[info]   ".blue().to_string(),
    }
}

/// Confidence tag (for `likely` — with a marker).
fn confidence_tag(conf: Confidence, lang: Lang) -> String {
    match conf {
        Confidence::Certain => String::new(),
        Confidence::Likely => format!(" {}", render_chrome(&Chrome::TagLikely, lang).dimmed()),
    }
}

/// Prints a single finding: tag + location + message + related sites.
fn print_finding(f: &Finding, lang: Lang) {
    let breadcrumb = breadcrumb(f);
    println!(
        "  {} {} ({}){}",
        severity_tag(f.severity),
        breadcrumb.bold(),
        f.rule.dimmed(),
        confidence_tag(f.confidence, lang)
    );
    println!("      {}", render(&f.message, lang));
    if !f.references.is_empty() {
        let refs: Vec<String> = f
            .references
            .iter()
            .map(|r| location_string(&r.file, &r.path.to_string()))
            .collect();
        println!(
            "      {} {}",
            render_chrome(&Chrome::Related, lang).dimmed(),
            refs.join(", ").dimmed()
        );
    }
}

/// The finding's main breadcrumb: file + logical path (if any).
fn breadcrumb(f: &Finding) -> String {
    location_string(&f.location.file, &f.location.path.to_string())
}

/// Builds the location string: `file` or `file > path`.
fn location_string(file: &camino::Utf8Path, path: &str) -> String {
    if path.is_empty() {
        file.to_string()
    } else {
        format!("{file} > {path}")
    }
}

/// Prints the summary banner with per-level counters.
fn print_summary(report: &Report, lang: Lang) {
    let s = &report.summary;
    println!();
    // Numbers are colored separately, so we take the template with already-substituted
    // string numbers and overlay color on them via the catalog-neutral
    // prefix "Итог:" / "Total:".
    let label = render_chrome(&Chrome::SummaryPrefix, lang);
    let body = render_chrome(
        &Chrome::Summary {
            errors: s.errors,
            warnings: s.warnings,
            infos: s.infos,
        },
        lang,
    );
    println!("  {} {}", label.bold(), colorize_summary(&body, s, lang));
    println!();
}

/// Colors the numbers in the summary line (errors in red, etc.).
///
/// The catalog provides neutral text; coloring is the renderer's job. Numbers in the
/// template come in a fixed order (errors, warnings, infos), so we
/// replace them one by one, advancing the cursor after each replacement — this works
/// correctly even when the counters are equal.
fn colorize_summary(body: &str, s: &dk_doctor_core::Summary, _lang: Lang) -> String {
    let colored = [
        (s.errors.to_string(), s.errors.to_string().red().to_string()),
        (
            s.warnings.to_string(),
            s.warnings.to_string().yellow().to_string(),
        ),
        (s.infos.to_string(), s.infos.to_string().blue().to_string()),
    ];
    let mut out = String::new();
    let mut rest = body;
    for (plain, painted) in colored {
        if let Some(pos) = rest.find(&plain) {
            out.push_str(&rest[..pos]);
            out.push_str(&painted);
            rest = &rest[pos + plain.len()..];
        }
    }
    out.push_str(rest);
    out
}
