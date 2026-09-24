use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorSession {
    pub photographer_id: String,
    pub name: String,
    pub role: String,
    pub access_token: String, // must serialize -- this is exactly what crosses the Tauri IPC
                               // boundary to JS, which then sends it as the Bearer token; it is
                               // NEVER written to disk (see StoredRefreshState below, which has no
                               // access_token field at all -- that's where anything skip_serializing
                               // would actually matter).
}

/// What's actually persisted to disk between app launches -- only the long-lived refresh token
/// plus enough identity to call RefreshDesktopToken; the access token itself is never written to
/// disk (kept in memory only, for the running session's lifetime).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredRefreshState {
    photographer_id: String,
    refresh_token: String,
}

fn session_file_path(app_data_dir: &PathBuf) -> PathBuf {
    app_data_dir.join("editor_session.json")
}

const API_BASE_URL: &str = "https://apis.vsnapu.com";

#[derive(Serialize)]
struct LoginRequestBody {
    mobile: String,
    password: String,
}

#[derive(Deserialize)]
struct LoginResponseBody {
    role: String,
    name: Option<String>,
    #[serde(rename = "photographerId")]
    photographer_id: String,
    token: Option<String>,
    #[serde(rename = "desktopRefreshToken")]
    desktop_refresh_token: Option<String>,
}

#[derive(Serialize)]
struct RefreshRequestBody {
    #[serde(rename = "photographerId")]
    photographer_id: String,
    #[serde(rename = "refreshToken")]
    refresh_token: String,
}

#[derive(Deserialize)]
struct RefreshResponseBody {
    token: String,
    #[serde(rename = "desktopRefreshToken")]
    desktop_refresh_token: String,
}

#[derive(Serialize)]
struct LogoutRequestBody {
    #[serde(rename = "photographerId")]
    photographer_id: String,
    // Proof of possession -- the backend only clears the session when this matches the stored
    // desktop refresh token, so knowing an editor's photographerId alone can't log them out.
    #[serde(rename = "refreshToken")]
    refresh_token: String,
}

fn read_stored_refresh_state(app_data_dir: &PathBuf) -> Option<StoredRefreshState> {
    let contents = std::fs::read_to_string(session_file_path(app_data_dir)).ok()?;
    serde_json::from_str(&contents).ok()
}

fn write_stored_refresh_state(app_data_dir: &PathBuf, state: &StoredRefreshState) -> Result<(), String> {
    std::fs::create_dir_all(app_data_dir).map_err(|e| format!("Could not create app data directory: {e}"))?;
    let json = serde_json::to_string(state).map_err(|e| format!("Could not serialize session: {e}"))?;
    std::fs::write(session_file_path(app_data_dir), json).map_err(|e| format!("Could not save session: {e}"))
}

fn clear_stored_refresh_state(app_data_dir: &PathBuf) {
    let _ = std::fs::remove_file(session_file_path(app_data_dir));
}

/// The backend returns login failures as a plain string body (ASP.NET `Unauthorized("...")`), which
/// arrives as a JSON string; surface that message, else a non-JSON-object raw body, else a fixed fallback.
fn extract_login_error(status: &str, body: &str) -> String {
    if let Ok(message) = serde_json::from_str::<String>(body) {
        if !message.trim().is_empty() {
            return message;
        }
    }
    let raw = body.trim();
    if !raw.is_empty() && !raw.starts_with('{') && !raw.starts_with('[') {
        return raw.to_string();
    }
    format!("Login failed (HTTP {status}). Check your mobile number and password.")
}

/// Only "Editor" and "VideoEditor" logins are eligible for the desktop bypass -- a "Photographer"
/// account can still log in (same endpoint, same credentials check) but never receives a
/// desktopRefreshToken from the backend, so login for that role is rejected here rather than
/// silently storing a session that can never be refreshed past its 7-day access token.
pub async fn login(app_data_dir: &PathBuf, mobile: &str, password: &str) -> Result<EditorSession, String> {
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{API_BASE_URL}/api/Photographer/Login"))
        .json(&LoginRequestBody { mobile: mobile.to_string(), password: password.to_string() })
        .send()
        .await
        .map_err(|e| format!("Could not reach the login server: {e}"))?;

    if !response.status().is_success() {
        let status = response.status().to_string();
        let body = response.text().await.unwrap_or_default();
        return Err(extract_login_error(&status, &body));
    }

    let body: LoginResponseBody = response
        .json()
        .await
        .map_err(|e| format!("Login response was not valid JSON: {e}"))?;

    if body.role != "Editor" && body.role != "VideoEditor" {
        return Err("This login is for editors only.".to_string());
    }

    let (Some(access_token), Some(refresh_token)) = (body.token, body.desktop_refresh_token) else {
        return Err("Login succeeded but did not return a desktop session token.".to_string());
    };

    write_stored_refresh_state(app_data_dir, &StoredRefreshState {
        photographer_id: body.photographer_id.clone(),
        refresh_token,
    })?;

    Ok(EditorSession {
        photographer_id: body.photographer_id,
        name: body.name.unwrap_or_default(),
        role: body.role,
        access_token,
    })
}

/// Called on app startup (and can be re-called near access-token expiry): silently exchanges the
/// stored refresh token for a fresh access token. Returns None (not an error) when there is no
/// stored session at all -- that's the ordinary logged-out state, not a failure. Returns Err only
/// when a session WAS stored but the server rejected it (expired/rotated/deactivated) -- the
/// caller clears local state in that case so the UI falls back to the logged-out "Login" link.
pub async fn refresh(app_data_dir: &PathBuf) -> Result<Option<EditorSession>, String> {
    let Some(stored) = read_stored_refresh_state(app_data_dir) else {
        return Ok(None);
    };

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{API_BASE_URL}/api/Photographer/RefreshDesktopToken"))
        .json(&RefreshRequestBody {
            photographer_id: stored.photographer_id.clone(),
            refresh_token: stored.refresh_token,
        })
        .send()
        .await
        .map_err(|e| format!("Could not reach the login server: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            clear_stored_refresh_state(app_data_dir);
            return Err("Your editor session has expired. Please log in again.".to_string());
        }
        return Err(format!("Could not refresh your editor session right now (HTTP {status}). Try again in a moment."));
    }

    let body: RefreshResponseBody = response
        .json()
        .await
        .map_err(|e| format!("Refresh response was not valid JSON: {e}"))?;

    write_stored_refresh_state(app_data_dir, &StoredRefreshState {
        photographer_id: stored.photographer_id.clone(),
        refresh_token: body.desktop_refresh_token,
    })?;

    Ok(Some(EditorSession {
        photographer_id: stored.photographer_id,
        name: String::new(), // Not returned by RefreshDesktopToken -- the caller already knows it from the prior login/refresh, so the JS side keeps its own displayed name across a silent refresh.
        role: String::new(),
        access_token: body.token,
    }))
}

pub async fn logout(app_data_dir: &PathBuf) -> Result<(), String> {
    if let Some(stored) = read_stored_refresh_state(app_data_dir) {
        let client = reqwest::Client::new();
        let _ = client
            .post(format!("{API_BASE_URL}/api/Photographer/LogoutDesktop"))
            .json(&LogoutRequestBody { photographer_id: stored.photographer_id, refresh_token: stored.refresh_token })
            .send()
            .await; // best-effort -- local state is cleared regardless of whether this reaches the server
    }

    clear_stored_refresh_state(app_data_dir);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vsnapu-downloader-session-test-{}", uuid_like()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn uuid_like() -> String {
        format!("{:?}", std::time::SystemTime::now())
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect()
    }

    #[test]
    fn session_file_path_is_inside_app_data_dir() {
        let dir = temp_dir();
        let path = session_file_path(&dir);
        assert_eq!(path, dir.join("editor_session.json"));
    }

    #[test]
    fn stored_refresh_state_round_trips_through_json() {
        let state = StoredRefreshState {
            photographer_id: "EDITOR-1".to_string(),
            refresh_token: "some-refresh-token".to_string(),
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: StoredRefreshState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.photographer_id, "EDITOR-1");
        assert_eq!(parsed.refresh_token, "some-refresh-token");
    }

    #[test]
    fn login_response_accepts_null_name() {
        let json = r#"{"role":"Editor","name":null,"photographerId":"E1","token":"t","desktopRefreshToken":"r"}"#;
        let body: LoginResponseBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.name.unwrap_or_default(), "");
    }

    #[test]
    fn extract_login_error_uses_json_string_body() {
        assert_eq!(extract_login_error("401 Unauthorized", "\"Invalid password.\""), "Invalid password.");
    }

    #[test]
    fn extract_login_error_uses_plain_body() {
        assert_eq!(extract_login_error("400 Bad Request", "  Password not set.  "), "Password not set.");
    }

    #[test]
    fn extract_login_error_falls_back_on_empty_or_object_body() {
        let expected = "Login failed (HTTP 500 Internal Server Error). Check your mobile number and password.";
        assert_eq!(extract_login_error("500 Internal Server Error", ""), expected);
        assert_eq!(extract_login_error("500 Internal Server Error", "{\"title\":\"x\"}"), expected);
    }
}
