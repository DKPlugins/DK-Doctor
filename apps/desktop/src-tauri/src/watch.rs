//! Watch Mode: a native filesystem watcher over the open project's `data/`
//! folder. On a debounced change it emits a `project-changed` event to the
//! frontend, which re-runs the scan. Only one project is watched at a time — a
//! new `watch_project` replaces the previous watcher; `unwatch_project` clears it.
//!
//! dk-doctor never writes to the project, so the watcher cannot feed back on its
//! own re-scans.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use tauri::{AppHandle, Emitter, Manager};

/// Holds the active watcher (dropping it stops the OS watch thread).
#[derive(Default)]
pub struct WatchState(pub Mutex<Option<Debouncer<RecommendedWatcher>>>);

/// Resolves the folder to watch: `<root>/data`, else `<root>/www/data`, else the
/// root itself (a watch on the whole project is still useful if the layout is odd).
fn watch_dir(root: &str) -> PathBuf {
    let base = Path::new(root);
    let data = base.join("data");
    if data.is_dir() {
        return data;
    }
    let www = base.join("www").join("data");
    if www.is_dir() {
        return www;
    }
    base.to_path_buf()
}

/// Starts watching the given project for changes (replacing any prior watcher).
///
/// Debounced at 600 ms so a burst of editor saves triggers one re-scan. The
/// event payload is the watched project path, so the frontend can ignore a stale
/// event after the user switched projects.
#[tauri::command]
pub fn watch_project(app: AppHandle, path: String) -> Result<(), String> {
    let dir = watch_dir(&path);
    let handle = app.clone();
    let payload = path.clone();
    let mut debouncer = new_debouncer(
        Duration::from_millis(600),
        move |res: DebounceEventResult| {
            if matches!(res, Ok(ref events) if !events.is_empty()) {
                let _ = handle.emit("project-changed", payload.clone());
            }
        },
    )
    .map_err(|e| e.to_string())?;

    debouncer
        .watcher()
        .watch(&dir, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;

    // Replacing the Option drops the previous debouncer → its watch thread stops.
    *app.state::<WatchState>()
        .0
        .lock()
        .map_err(|e| e.to_string())? = Some(debouncer);
    Ok(())
}

/// Stops watching the current project (no-op if nothing is being watched).
#[tauri::command]
pub fn unwatch_project(app: AppHandle) -> Result<(), String> {
    *app.state::<WatchState>()
        .0
        .lock()
        .map_err(|e| e.to_string())? = None;
    Ok(())
}
