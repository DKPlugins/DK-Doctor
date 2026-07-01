//! Library of the dk-doctor desktop application: WebView window + the `analyze`
//! command, embedding the analyzer as a library (no subprocess/IPC).
//!
//! `main.rs` calls [`run`]; this form (lib + thin bin) is canonical for
//! Tauri v2 and compatible with the mobile entry point.

pub mod analyze;
pub mod report_json;
pub mod watch;

/// Writes a text file to an absolute path (for exporting the report).
///
/// The path is chosen by the user via the system save dialog on the frontend
/// side, so the command merely writes an already-approved path. I/O errors are
/// returned as a string for display in a toast.
#[tauri::command]
fn write_text_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, contents).map_err(|e| format!("{path}: {e}"))
}

/// Writes a binary file to an absolute path (for exporting a map PNG).
///
/// As with [`write_text_file`], the path is chosen by the user via the system
/// save dialog on the frontend, so the command merely writes an approved path.
#[tauri::command]
fn write_binary_file(path: String, bytes: Vec<u8>) -> Result<(), String> {
    std::fs::write(&path, bytes).map_err(|e| format!("{path}: {e}"))
}

/// Opens an `https` link in the system browser (for the update notification).
///
/// Accepts only `https://` URLs (we reject `file://`/arbitrary schemes); the
/// opener itself is the OS's built-in one, with no third-party dependencies.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("only https URLs are allowed".to_string());
    }
    let result = if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(&url).spawn()
    };
    result.map(|_| ()).map_err(|e| e.to_string())
}

/// Launches the application: registers the dialog plugin and the
/// `scan`/`analyze`/`write_text_file` commands, then brings up the WebView window.
///
/// UI settings (theme, language) and the list of recent projects are stored by
/// the frontend in `localStorage` (offline, with no external dependencies);
/// `dialog` provides the system folder picker and the export save dialog.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Manager;

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(watch::WatchState::default())
        .invoke_handler(tauri::generate_handler![
            analyze::scan,
            analyze::analyze,
            analyze::map_atlas,
            analyze::map_render,
            analyze::map_graph,
            analyze::event_commands,
            analyze::read_project_image,
            watch::watch_project,
            watch::unwatch_project,
            write_text_file,
            write_binary_file,
            open_url
        ])
        // The window is created hidden (visible:false) and shown by the frontend
        // after the first frame (anti-flash). If the frontend fails to run (WebView2
        // crash, an error during module initialization), we show the window from
        // Rust after a timeout, so the user does not end up with an "invisible"
        // process without any feedback.
        .setup(|app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(3));
                if let Some(w) = handle.get_webview_window("main") {
                    if !w.is_visible().unwrap_or(true) {
                        let _ = w.show();
                    }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running dk-doctor desktop");
}
