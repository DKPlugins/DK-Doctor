//! Контрактный тест: JSON команды `analyze` БАЙТ-В-БАЙТ совпадает с
//! `dk-doctor --format json` на том же фикстуре.
//!
//! Запускается без webview — вызывает чистую функцию-конвейер напрямую и
//! сверяет с эталоном, снятым из CLI (через `cargo run -p dk-doctor`).
//! Так подтверждается, что десктоп не форкнул контракт.

use std::process::Command;

/// Путь к корню фикстуры MZ относительно манифеста (apps/desktop/src-tauri).
const FIXTURE: &str = "../../../testdata/mz-fixture";

/// Прогоняет CLI и возвращает его stdout (JSON-артефакт) для данного языка.
fn cli_json(lang: &str) -> String {
    let out = Command::new(env!("CARGO"))
        .args([
            "run",
            "-q",
            "-p",
            "dk-doctor",
            "--",
            "testdata/mz-fixture",
            "--format",
            "json",
            "--lang",
            lang,
        ])
        .current_dir("../../..")
        .output()
        .expect("run dk-doctor CLI");
    String::from_utf8(out.stdout).expect("CLI stdout utf8")
}

/// Сверяет вывод `analyze` (через встроенный конвейер) с CLI для одного языка.
fn assert_matches(lang: &str) {
    // `analyze` теперь async (тело уходит на blocking-пул) — гоним его на
    // tauri-runtime, чтобы `spawn_blocking` имел контекст исполнителя.
    let desktop = tauri::async_runtime::block_on(dk_doctor_desktop_lib::analyze::analyze(
        FIXTURE.to_string(),
        Some(lang.to_string()),
        None,
    ))
    .expect("desktop analyze ok");

    let cli = cli_json(lang);
    // CLI печатает через writeln! → завершающий перевод строки; артефакт тот же.
    assert_eq!(
        desktop.trim_end(),
        cli.trim_end(),
        "desktop JSON diverged from CLI for lang={lang}"
    );
}

#[test]
fn json_byte_identical_to_cli_en() {
    assert_matches("en");
}

#[test]
fn json_byte_identical_to_cli_ru() {
    assert_matches("ru");
}
