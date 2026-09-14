use crate::manifest::{Manifest, ManifestFile};
use futures_util::StreamExt;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio::sync::Semaphore;

const MAX_CONCURRENT_DOWNLOADS: usize = 4;

#[derive(Clone, Serialize)]
pub struct DownloadProgressEvent {
    #[serde(rename = "fileName")]
    pub file_name: String,
    #[serde(rename = "bytesDownloaded")]
    pub bytes_downloaded: u64,
    #[serde(rename = "totalBytes")]
    pub total_bytes: u64,
    pub done: bool,
    pub error: Option<String>,
}

/// Given how many bytes of a file already exist locally and the file's total size, returns the
/// inclusive byte range still needed as an HTTP Range header value (start, end), or None if
/// nothing more needs downloading. Defensive against a stale/corrupted partial file being larger
/// than the real total (returns None rather than requesting a negative range).
pub fn compute_resume_range(existing_bytes: u64, total_bytes: u64) -> Option<(u64, u64)> {
    if existing_bytes >= total_bytes {
        return None;
    }
    Some((existing_bytes, total_bytes.saturating_sub(1)))
}

/// Strips any path-separator components from a server-provided file name before it's joined to
/// the user's chosen destination directory. Defense in depth: the manifest is server-trusted per
/// the app's design, but a file name is still attacker-adjacent input (it flows from whatever the
/// job's uploaded files were named), and this makes a `../../` name inert rather than relying
/// solely on server-side sanitization.
pub fn sanitize_file_name(name: &str) -> String {
    // Split on both `/` and `\` explicitly (rather than relying on `Path::file_name`, which only
    // treats `\` as a separator on Windows) so a server-provided name is sanitized the same way
    // regardless of which OS this app is running on.
    let candidate = name.rsplit(['/', '\\']).next().unwrap_or("").to_string();

    if candidate.is_empty() {
        "download".to_string()
    } else {
        candidate
    }
}

async fn download_one_file(
    client: reqwest::Client,
    file: ManifestFile,
    destination_dir: PathBuf,
    app: AppHandle,
) -> Result<(), String> {
    let safe_name = sanitize_file_name(&file.file_name);
    let destination_path = destination_dir.join(&safe_name);

    let existing_bytes = tokio::fs::metadata(&destination_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    let range = compute_resume_range(existing_bytes, file.size_bytes);

    if range.is_none() {
        emit_progress(&app, &safe_name, file.size_bytes, file.size_bytes, true, None);
        return Ok(());
    }

    let (start, end) = range.unwrap();
    let mut request = client.get(&file.url);
    if start > 0 {
        request = request.header("Range", format!("bytes={start}-{end}"));
    }

    let response = match request.send().await {
        Ok(r) => r,
        Err(e) => {
            let message = format!("Download failed for {safe_name}: {e}");
            emit_progress(&app, &safe_name, existing_bytes, file.size_bytes, true, Some(message.clone()));
            return Err(message);
        }
    };

    if !response.status().is_success() {
        let message = format!("Download failed for {safe_name}: HTTP {}", response.status());
        emit_progress(&app, &safe_name, existing_bytes, file.size_bytes, true, Some(message.clone()));
        return Err(message);
    }

    let mut file_handle = match OpenOptions::new()
        .create(true)
        .write(true)
        .open(&destination_path)
        .await
    {
        Ok(f) => f,
        Err(e) => {
            let message = format!("Could not open {safe_name} for writing: {e}");
            emit_progress(&app, &safe_name, existing_bytes, file.size_bytes, true, Some(message.clone()));
            return Err(message);
        }
    };

    if let Err(e) = file_handle.seek(std::io::SeekFrom::Start(start)).await {
        let message = format!("Could not seek in {safe_name}: {e}");
        emit_progress(&app, &safe_name, existing_bytes, file.size_bytes, true, Some(message.clone()));
        return Err(message);
    }

    let mut downloaded = start;
    let mut stream = response.bytes_stream();
    while let Some(chunk_result) = stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(e) => {
                let message = format!("Connection dropped while downloading {safe_name}: {e}. Run the download again to resume.");
                emit_progress(&app, &safe_name, downloaded, file.size_bytes, true, Some(message.clone()));
                return Err(message);
            }
        };

        if let Err(e) = file_handle.write_all(&chunk).await {
            let message = format!("Could not write {safe_name} to disk: {e}");
            emit_progress(&app, &safe_name, downloaded, file.size_bytes, true, Some(message.clone()));
            return Err(message);
        }

        downloaded += chunk.len() as u64;
        emit_progress(&app, &safe_name, downloaded, file.size_bytes, false, None);
    }

    emit_progress(&app, &safe_name, downloaded, file.size_bytes, true, None);
    Ok(())
}

fn emit_progress(app: &AppHandle, file_name: &str, bytes_downloaded: u64, total_bytes: u64, done: bool, error: Option<String>) {
    let _ = app.emit(
        "download-progress",
        DownloadProgressEvent {
            file_name: file_name.to_string(),
            bytes_downloaded,
            total_bytes,
            done,
            error,
        },
    );
}

pub async fn download_all(manifest: Manifest, destination_dir: PathBuf, app: AppHandle) -> Result<(), String> {
    tokio::fs::create_dir_all(&destination_dir)
        .await
        .map_err(|e| format!("Could not create destination folder: {e}"))?;

    let client = reqwest::Client::new();
    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));
    let mut handles = Vec::new();

    for file in manifest.files {
        let permit_semaphore = Arc::clone(&semaphore);
        let client = client.clone();
        let destination_dir = destination_dir.clone();
        let app = app.clone();

        handles.push(tokio::spawn(async move {
            let _permit = permit_semaphore.acquire().await;
            download_one_file(client, file, destination_dir, app).await
        }));
    }

    let mut first_error = None;
    for handle in handles {
        match handle.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                first_error.get_or_insert(e);
            }
            Err(join_error) => {
                first_error.get_or_insert(format!("Download task panicked: {join_error}"));
            }
        };
    }

    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_resume_range_no_existing_bytes_downloads_whole_file() {
        assert_eq!(compute_resume_range(0, 1000), Some((0, 999)));
    }

    #[test]
    fn compute_resume_range_partial_existing_bytes_resumes_from_there() {
        assert_eq!(compute_resume_range(400, 1000), Some((400, 999)));
    }

    #[test]
    fn compute_resume_range_already_complete_returns_none() {
        assert_eq!(compute_resume_range(1000, 1000), None);
    }

    #[test]
    fn compute_resume_range_existing_bytes_exceed_total_returns_none() {
        // Defensive: a corrupted/truncated total shouldn't request a negative range.
        assert_eq!(compute_resume_range(1200, 1000), None);
    }

    #[test]
    fn sanitize_file_name_strips_path_separators() {
        assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("sub\\dir\\file.jpg"), "file.jpg");
    }

    #[test]
    fn sanitize_file_name_leaves_a_plain_name_unchanged() {
        assert_eq!(sanitize_file_name("IMG_0001.jpg"), "IMG_0001.jpg");
    }

    #[test]
    fn sanitize_file_name_empty_input_returns_placeholder() {
        assert_eq!(sanitize_file_name(""), "download");
    }
}
