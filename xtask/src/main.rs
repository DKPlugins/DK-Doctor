//! `xtask` — developer tasks for dk-doctor.
//!
//! `mine-plugin-profile <path>` distils a plugin `.js` (or every plugin in a
//! directory) into a curated-profile skeleton (commented TOML) for a human to
//! review before dropping into `<project>/.dk-doctor/plugins/`. It reuses the
//! adapter's Tier-A/Tier-B analyzers — the game is never run.

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "xtask", about = "dk-doctor developer tasks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Mine a plugin `.js` file or a directory of plugins into profile skeleton(s).
    MinePluginProfile {
        /// Path to a plugin `.js` file or a folder (e.g. `js/plugins`).
        path: PathBuf,
        /// Write `<name>.toml` into this folder instead of printing to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    match Cli::parse().cmd {
        Cmd::MinePluginProfile { path, out } => mine_cmd(&path, out.as_deref()),
    }
}

/// `true` for a real plugin `.js` — excludes engine core (`rmmz_*`/`rpg_*`),
/// bootstrap (`main`/`plugins`) and vendored libraries (`js/libs/`).
fn is_plugin_js(path: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("js") {
        return false;
    }
    if path
        .components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case("libs"))
    {
        return false;
    }
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    !(stem.starts_with("rmmz_") || stem.starts_with("rpg_") || stem == "main" || stem == "plugins")
}

/// Collects the plugin `.js` files under `path` (the file itself, or a directory walk).
fn collect_js(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(Result::ok)
        .map(|e| e.into_path())
        .filter(|p| p.is_file() && is_plugin_js(p))
        .collect();
    files.sort();
    files
}

fn mine_cmd(path: &Path, out: Option<&Path>) -> ExitCode {
    if !path.exists() {
        eprintln!("путь не найден: {}", path.display());
        return ExitCode::FAILURE;
    }
    let files = collect_js(path);
    if files.is_empty() {
        eprintln!("не найдено плагинов (.js) по пути: {}", path.display());
        return ExitCode::FAILURE;
    }
    if let Some(dir) = out
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        eprintln!("не удалось создать {}: {e}", dir.display());
        return ExitCode::FAILURE;
    }

    let mut mined = 0usize;
    for file in &files {
        let src = match std::fs::read_to_string(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("пропущен {}: {e}", file.display());
                continue;
            }
        };
        let name = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Plugin");
        let skeleton = dk_doctor_rpgmaker::mine_plugin_profile(name, &src);
        match out {
            Some(dir) => {
                let dst = dir.join(format!("{name}.toml"));
                if let Err(e) = std::fs::write(&dst, &skeleton) {
                    eprintln!("не удалось записать {}: {e}", dst.display());
                    continue;
                }
                println!("записан {}", dst.display());
            }
            None => {
                if mined > 0 {
                    println!();
                }
                println!("# ===== {name} =====");
                print!("{skeleton}");
            }
        }
        mined += 1;
    }
    if mined == 0 {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
