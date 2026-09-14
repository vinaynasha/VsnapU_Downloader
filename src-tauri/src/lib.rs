mod download;
mod manifest;

use manifest::Manifest;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

#[tauri::command]
async fn fetch_manifest_command(manifest_url: String) -> Result<Manifest, String> {
    manifest::fetch_manifest(&manifest_url).await
}

#[tauri::command]
async fn download_all_command(manifest: Manifest, destination_dir: String, app: AppHandle) -> Result<(), String> {
    download::download_all(manifest, PathBuf::from(destination_dir), app).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // On Windows/Linux, a second app launch (e.g. clicking the download link again, or the
            // OS launching a new process for a vsnapu-download:// URL) arrives here instead of via
            // the deep-link plugin's onOpenUrl event (that event is macOS/iOS/Android-only -- see
            // tauri-plugin-deep-link's own README). Forward any vsnapu-download:// URL found in the
            // new instance's launch arguments to the frontend as a custom event.
            if let Some(url) = argv.iter().find(|arg| arg.starts_with("vsnapu-download://")) {
                let _ = app.emit("deep-link-url", url.clone());
            }
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![fetch_manifest_command, download_all_command])
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
