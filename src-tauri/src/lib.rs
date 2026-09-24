mod download;
mod editor_session;
mod manifest;

use manifest::Manifest;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

#[tauri::command]
async fn fetch_manifest_command(manifest_url: String) -> Result<Manifest, String> {
    manifest::fetch_manifest(&manifest_url).await
}

#[tauri::command]
async fn download_all_command(manifest: Manifest, destination_dir: String, app: AppHandle) -> Result<(), String> {
    download::download_all(manifest, PathBuf::from(destination_dir), app).await
}

#[tauri::command]
async fn editor_login_command(mobile: String, password: String, app: AppHandle) -> Result<editor_session::EditorSession, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| format!("Could not resolve app data directory: {e}"))?;
    editor_session::login(&app_data_dir, &mobile, &password).await
}

#[tauri::command]
async fn editor_refresh_command(app: AppHandle) -> Result<Option<editor_session::EditorSession>, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| format!("Could not resolve app data directory: {e}"))?;
    editor_session::refresh(&app_data_dir).await
}

#[tauri::command]
async fn editor_logout_command(app: AppHandle) -> Result<(), String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| format!("Could not resolve app data directory: {e}"))?;
    editor_session::logout(&app_data_dir).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {
            // With the `deep-link` Cargo feature enabled (see Cargo.toml), this plugin automatically
            // forwards a second instance's launch arguments into tauri-plugin-deep-link's
            // handle_cli_arguments, which emits the same `deep-link://new-url` event the existing
            // `onOpenUrl` listener in main.js already subscribes to -- no manual argv handling is
            // needed (or wanted: doing so here previously caused the same URL to fire twice, once via
            // this plugin's automatic forwarding and once via a redundant custom event, launching two
            // concurrent downloads into the same job folder).
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            fetch_manifest_command,
            download_all_command,
            editor_login_command,
            editor_refresh_command,
            editor_logout_command
        ])
        .setup(|app| {
            use tauri_plugin_deep_link::DeepLinkExt;
            // Registering the vsnapu-download:// URL scheme can fail (e.g. "unsupported platform" for
            // an unbundled dev/debug binary, or a signing/permission issue in a bundled release build).
            // That failure must never crash the whole app -- deep-link handoff not being registered
            // yet is a real but recoverable condition (the user can still open the app directly, and a
            // packaged, signed release build is expected to register successfully), whereas a `?` here
            // would propagate out of this Tauri `.setup()` hook and abort the entire process before any
            // window ever opens, which is what actually happened on a real machine.
            if let Err(e) = app.deep_link().register("vsnapu-download") {
                eprintln!("Warning: failed to register the vsnapu-download:// URL scheme: {e}. The app will still start, but launching it via a download link may not work until this is resolved.");
            }

            // Covers two cold-start cases the runtime onOpenUrl/single-instance-callback events
            // above can miss: (1) macOS -- the deep-link plugin's own docs recommend calling
            // get_current() on startup since onOpenUrl can fire before the frontend finishes
            // loading; (2) Windows -- this very process (not a second instance) may itself have
            // been launched directly with the URL as a CLI argument.
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                if let Some(url) = urls.first() {
                    let _ = app.emit("deep-link-url", url.to_string());
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the VSnapU Downloader application");
}
