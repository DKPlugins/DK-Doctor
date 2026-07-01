//! End-to-end tests for the CI gate: run the compiled `dk-doctor` binary on the
//! synthetic MZ fixture and assert `--fail-on` / `--baseline` / `--write-baseline`
//! drive the process exit code as documented.

use std::process::Command;

/// Path to the MZ fixture project.
fn fixture() -> String {
    camino::Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("testdata")
        .join("mz-fixture")
        .to_string()
}

/// Runs the binary with the given args and returns its exit code.
fn run(args: &[&str]) -> i32 {
    Command::new(env!("CARGO_BIN_EXE_dk-doctor"))
        .args(args)
        .output()
        .expect("binary runs")
        .status
        .code()
        .expect("exit code")
}

/// Runs the binary and returns its captured stdout as a string.
fn run_stdout(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_dk-doctor"))
        .args(args)
        .output()
        .expect("binary runs");
    String::from_utf8(out.stdout).expect("utf8 stdout")
}

/// Extracts the first `"fingerprint": "<hex>"` value from a JSON report.
fn first_fingerprint(json: &str) -> String {
    let key = "\"fingerprint\": \"";
    let start = json.find(key).expect("a fingerprint field") + key.len();
    let rest = &json[start..];
    let end = rest.find('"').expect("closing quote");
    rest[..end].to_string()
}

#[test]
fn fail_on_never_always_passes() {
    // The fixture has errors, but `never` never gates.
    assert_eq!(run(&["--fail-on", "never", &fixture()]), 0);
}

#[test]
fn fail_on_error_trips_on_fixture_errors() {
    // The fixture plants referential-integrity / broken-transfer errors.
    assert_eq!(run(&["--fail-on", "error", &fixture()]), 1);
}

#[test]
fn default_exit_code_is_two_on_errors() {
    // Without --fail-on, the legacy severity mapping applies (errors → 2).
    assert_eq!(run(&[&fixture()]), 2);
}

#[test]
fn write_baseline_then_fail_on_new_passes() {
    // Record the current findings as the baseline, then `--fail-on new` sees
    // nothing new → exit 0. Uses a per-process temp path to stay isolated.
    let baseline = std::env::temp_dir().join(format!("dkbaseline_{}.json", std::process::id()));
    let baseline = camino::Utf8PathBuf::from_path_buf(baseline).unwrap();

    let write = run(&["--write-baseline", baseline.as_str(), &fixture()]);
    assert_eq!(write, 0, "writing a baseline exits 0");
    assert!(baseline.as_std_path().exists(), "baseline file created");

    let gate = run(&[
        "--fail-on",
        "new",
        "--baseline",
        baseline.as_str(),
        &fixture(),
    ]);
    assert_eq!(
        gate, 0,
        "no new findings vs the just-written baseline → pass"
    );

    let _ = std::fs::remove_file(baseline.as_std_path());
}

#[test]
fn config_suppress_removes_a_finding_by_fingerprint() {
    // Grab a real fingerprint from the JSON report, then suppress it via a config
    // passed with --config, and assert it disappears from the next run.
    let json = run_stdout(&["--format", "json", &fixture()]);
    let fp = first_fingerprint(&json);
    let before = json.matches("\"fingerprint\":").count();
    assert!(before >= 1);

    let cfg = std::env::temp_dir().join(format!("dksup_{}.toml", std::process::id()));
    let cfg = camino::Utf8PathBuf::from_path_buf(cfg).unwrap();
    std::fs::write(
        cfg.as_std_path(),
        format!("[[suppress]]\nfingerprint = \"{fp}\"\nreason = \"test\"\n"),
    )
    .unwrap();

    let json2 = run_stdout(&["--config", cfg.as_str(), "--format", "json", &fixture()]);
    let after = json2.matches("\"fingerprint\":").count();
    assert!(!json2.contains(&fp), "the suppressed fingerprint is gone");
    assert_eq!(after, before - 1, "exactly one finding suppressed");

    let _ = std::fs::remove_file(cfg.as_std_path());
}

#[test]
fn fail_on_new_without_baseline_treats_all_as_new() {
    // With no baseline file, every finding is "new" → the gate trips.
    let missing =
        std::env::temp_dir().join(format!("dkbaseline_absent_{}.json", std::process::id()));
    let missing = camino::Utf8PathBuf::from_path_buf(missing).unwrap();
    let _ = std::fs::remove_file(missing.as_std_path());
    assert_eq!(
        run(&[
            "--fail-on",
            "new",
            "--baseline",
            missing.as_str(),
            &fixture()
        ]),
        1
    );
}
