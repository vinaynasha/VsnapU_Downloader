mod download;
mod manifest;

use manifest::Manifest;
use std::path::PathBuf;
use tauri::AppHandle;

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
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![fetch_manifest_command, download_all_command])
        .setup(|app| {
            use tauri_plugin_deep_link::DeepLinkExt;
            app.deep_link().register("vsnapu-download")?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the VSnapU Downloader application");
}
