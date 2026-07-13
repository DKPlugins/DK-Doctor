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

/// Recursively copies a fixture directory (used to plant a project-local
/// `.dk-doctor.toml` without mutating the shared `testdata/` fixture).
fn copy_dir_recursive(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let target = dst.join(&name);
        let ft = entry.file_type()?;
        if ft.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else if ft.is_file() {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// The shared fixture path (absolute).
fn fixture_path() -> std::path::PathBuf {
    camino::Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("testdata")
        .join("mz-fixture")
        .into_std_path_buf()
}

/// Spawns a temp copy of the MZ fixture so a project-local `.dk-doctor.toml` (or
/// `.dk-doctor/` dir) can be planted without touching the shared fixture.
fn fixture_copy(tag: &str) -> camino::Utf8PathBuf {
    let dst = std::env::temp_dir().join(format!("dkdoc-nc-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dst);
    copy_dir_recursive(&fixture_path(), &dst).expect("fixture copy");
    camino::Utf8PathBuf::from_path_buf(dst).unwrap()
}

#[test]
fn no_project_config_ignores_project_local_suppressions() {
    // Threat model (#1): a malicious project ships `.dk-doctor.toml` with
    // `fail_on = "never"` and `[[suppress]]` entries for its own findings to make
    // the analyzer exit 0. Without `--no-project-config`, the auto-loaded project
    // config is honored (exit 0). With it, the config is ignored and the real
    // severity-based exit code (2, errors present) is restored.
    let project = fixture_copy("suppress");

    // First grab a real fingerprint to suppress.
    let json = run_stdout(&["--format", "json", project.as_str()]);
    let fp = first_fingerprint(&json);

    // Plant a hostile project config: fail_on never + suppress one fingerprint.
    std::fs::write(
        project.join(".dk-doctor.toml").as_std_path(),
        format!("fail_on = \"never\"\n[[suppress]]\nfingerprint = \"{fp}\"\nreason = \"hide\"\n",),
    )
    .unwrap();

    // Without the flag: config wins → exit 0 despite errors.
    assert_eq!(
        run(&[project.as_str()]),
        0,
        "project-local fail_on=never suppresses the gate (the vulnerability)"
    );

    // With --no-project-config: project config ignored → legacy severity exit
    // code (2, there are errors) and the suppressed finding is back.
    assert_eq!(
        run(&["--no-project-config", project.as_str()]),
        2,
        "--no-project-config ignores the hostile config"
    );
    let json2 = run_stdout(&["--no-project-config", "--format", "json", project.as_str()]);
    assert!(
        json2.contains(&fp),
        "the suppressed fingerprint reappears under --no-project-config"
    );

    let _ = std::fs::remove_dir_all(project.as_std_path());
}

#[test]
fn no_project_config_ignores_project_local_disable() {
    // A hostile project disables a noisy error rule via `.dk-doctor.toml`. Without
    // `--no-project-config` the rule is muted; with it, the rule runs again.
    let project = fixture_copy("disable");
    std::fs::write(
        project.join(".dk-doctor.toml").as_std_path(),
        "disable = [\"referential-integrity\"]\n",
    )
    .unwrap();

    // referential-integrity normally contributes 5 errors on this fixture. With
    // the project config disabling it, the JSON report has no such finding.
    let muted = run_stdout(&["--format", "json", project.as_str()]);
    let muted_count = muted.matches("referential-integrity").count();
    assert_eq!(
        muted_count, 0,
        "project-local disable mutes the rule (the vulnerability)"
    );

    // With --no-project-config the disable is ignored → the rule fires again.
    let restored = run_stdout(&["--no-project-config", "--format", "json", project.as_str()]);
    let restored_count = restored.matches("referential-integrity").count();
    assert!(
        restored_count >= 5,
        "--no-project-config restores the disabled rule ({restored_count} findings)"
    );

    let _ = std::fs::remove_dir_all(project.as_std_path());
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
