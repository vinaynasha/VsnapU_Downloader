use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFile {
    #[serde(rename = "fileName")]
    pub file_name: String,
    pub url: String,
    #[serde(rename = "sizeBytes")]
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(rename = "jobName")]
    pub job_name: String,
    #[serde(rename = "folderName")]
    pub folder_name: Option<String>,
    pub files: Vec<ManifestFile>,
}

#[derive(Deserialize)]
struct ErrorBody {
    message: Option<String>,
}

/// Reads a `{ "message": "..." }` field out of an error response body when present, falling back
/// to a generic "may have expired" message when the body isn't JSON or has no `message` field
/// (e.g. a raw gateway timeout page). Kept as a standalone function -- no HTTP involved -- so it
/// can be unit-tested directly against sample bodies.
fn extract_error_message(status: &str, body: &str) -> String {
    if let Ok(parsed) = serde_json::from_str::<ErrorBody>(body) {
        if let Some(message) = parsed.message {
            return message;
        }
    }
    format!("Manifest link rejected ({status}). It may have expired -- try downloading again from the web page.")
}

/// True only for URLs on our own API host. An Authorization header must never be attached to a
/// GCS-signed URL: GCS rejects unexpected auth headers (and it would leak the editor token).
pub(crate) fn should_attach_auth_header(url: &str) -> bool {
    url.starts_with("https://apis.vsnapu.com/")
}

pub async fn fetch_manifest(manifest_url: &str, access_token: Option<&str>) -> Result<Manifest, String> {
    let client = reqwest::Client::new();
    let mut request = client.get(manifest_url);
    if let Some(token) = access_token {
        if should_attach_auth_header(manifest_url) {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("Failed to reach the manifest link: {e}"))?;

    if !response.status().is_success() {
        let status = response.status().to_string();
        let body = response.text().await.unwrap_or_default();
        return Err(extract_error_message(&status, &body));
    }

    response
        .json::<Manifest>()
        .await
        .map_err(|e| format!("Manifest response was not valid JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_error_message_reads_message_field_when_present() {
        let body = r#"{"message":"This job unlocks at 100% payment received. 30% has been received so far. Please complete the remaining payment to download."}"#;
        assert_eq!(
            extract_error_message("HTTP 403", body),
            "This job unlocks at 100% payment received. 30% has been received so far. Please complete the remaining payment to download."
        );
    }

    #[test]
    fn extract_error_message_falls_back_when_body_is_not_json() {
        let body = "Bad Gateway";
        assert_eq!(
            extract_error_message("HTTP 502", body),
            "Manifest link rejected (HTTP 502). It may have expired -- try downloading again from the web page."
        );
    }

    #[test]
    fn extract_error_message_falls_back_when_json_has_no_message_field() {
        let body = r#"{"other":"value"}"#;
        assert_eq!(
            extract_error_message("HTTP 500", body),
            "Manifest link rejected (HTTP 500). It may have expired -- try downloading again from the web page."
        );
    }

    #[test]
    fn should_attach_auth_header_for_own_api_host() {
        assert!(should_attach_auth_header("https://apis.vsnapu.com/api/DirectDownload/Manifest?token=abc"));
    }

    #[test]
    fn should_attach_auth_header_false_for_external_host() {
        assert!(!should_attach_auth_header("https://storage.googleapis.com/some-bucket/some-object?X-Goog-Signature=abc"));
    }

    #[test]
    fn should_attach_auth_header_false_for_malformed_url() {
        assert!(!should_attach_auth_header("not-a-url"));
    }
}
