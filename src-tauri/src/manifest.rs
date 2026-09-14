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

pub async fn fetch_manifest(manifest_url: &str) -> Result<Manifest, String> {
    let response = reqwest::get(manifest_url)
        .await
        .map_err(|e| format!("Failed to reach the manifest link: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Manifest link rejected (HTTP {}). It may have expired -- try downloading again from the web page.", response.status()));
    }

    response
        .json::<Manifest>()
        .await
        .map_err(|e| format!("Manifest response was not valid JSON: {e}"))
}
