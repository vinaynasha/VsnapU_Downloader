use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateStatus {
    None,
    Banner,
    Blocking,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub status: UpdateStatus,
    pub latest_version: String,
    pub installer_url: String,
}

pub const ALLOWED_URL_PREFIX: &str = "https://github.com/vinaynasha/VsnapU_Downloader/";

pub fn parse_version(raw: &str) -> Option<Version> {
    let trimmed = raw.trim().trim_start_matches(|c: char| c == 'v' || c == 'V');
    // Keep only the leading digits-and-dots run so "0.1.6-beta.2" / "0.1.6+build5" parse as 0.1.6.
    let numeric: String = trimmed
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let parts: Vec<&str> = numeric.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some(Version(
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

/// Finds a `min-version: X.Y.Z` line in a release's notes. Only the FIRST matching line is used; an
/// unparsable value yields None (a later valid line is deliberately not consulted).
pub fn extract_min_version(body: &str) -> Option<Version> {
    for line in body.lines() {
        let cleaned = line.trim_start_matches(|c: char| c.is_whitespace() || c == '-' || c == '*' || c == '>');
        let lower = cleaned.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("min-version:") {
            return parse_version(rest);
        }
    }
    None
}

pub fn decide(current: Version, latest: Version, min_version: Option<Version>) -> UpdateStatus {
    // A minimum above the latest release can never be satisfied by updating -- ignore it rather than
    // lock every user out over a typo.
    let effective_min = min_version.filter(|min| *min <= latest);
    if let Some(min) = effective_min {
        if current < min {
            return UpdateStatus::Blocking;
        }
    }
    if latest > current {
        UpdateStatus::Banner
    } else {
        UpdateStatus::None
    }
}

pub fn is_allowed_update_url(url: &str) -> bool {
    url.starts_with(ALLOWED_URL_PREFIX)
}

pub fn pick_installer_url(release: &Release, os: &str) -> String {
    let suffix = match os {
        "windows" => ".msi",
        "macos" => ".dmg",
        _ => "",
    };
    if !suffix.is_empty() {
        if let Some(asset) = release
            .assets
            .iter()
            .find(|asset| asset.name.to_ascii_lowercase().ends_with(suffix))
        {
            return asset.browser_download_url.clone();
        }
    }
    release.html_url.clone()
}

impl UpdateCheckResult {
    pub fn none() -> Self {
        UpdateCheckResult {
            status: UpdateStatus::None,
            latest_version: String::new(),
            installer_url: String::new(),
        }
    }
}

pub fn evaluate(current_version: &str, os: &str, release: &Release) -> UpdateCheckResult {
    let (Some(current), Some(latest)) = (parse_version(current_version), parse_version(&release.tag_name)) else {
        return UpdateCheckResult::none();
    };

    let min_version = release.body.as_deref().and_then(extract_min_version);
    let status = decide(current, latest, min_version);
    if status == UpdateStatus::None {
        return UpdateCheckResult::none();
    }

    // Only ever hand out URLs inside our own repo; fall back to the release page, and if even that is
    // not ours there is nothing safe to open, so offer no update at all.
    let mut installer_url = pick_installer_url(release, os);
    if !is_allowed_update_url(&installer_url) {
        installer_url = release.html_url.clone();
    }
    if !is_allowed_update_url(&installer_url) {
        return UpdateCheckResult::none();
    }

    UpdateCheckResult {
        status,
        latest_version: format!("{}.{}.{}", latest.0, latest.1, latest.2),
        installer_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_json(tag: &str, body: Option<&str>) -> Release {
        let body_json = match body {
            Some(b) => format!("{:?}", b),
            None => "null".to_string(),
        };
        let json = format!(
            r#"{{
                "tag_name": "{tag}",
                "body": {body_json},
                "html_url": "https://github.com/vinaynasha/VsnapU_Downloader/releases/tag/{tag}",
                "assets": [
                    {{"name": "VSnapU.Downloader_x64_en-US.msi", "browser_download_url": "https://github.com/vinaynasha/VsnapU_Downloader/releases/download/{tag}/VSnapU.Downloader_x64_en-US.msi"}},
                    {{"name": "VSnapU.Downloader_x64.dmg", "browser_download_url": "https://github.com/vinaynasha/VsnapU_Downloader/releases/download/{tag}/VSnapU.Downloader_x64.dmg"}}
                ]
            }}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    // ---- parse_version ----

    #[test]
    fn parse_version_reads_plain_and_prefixed_versions() {
        assert_eq!(parse_version("0.1.6"), Some(Version(0, 1, 6)));
        assert_eq!(parse_version("v0.1.6"), Some(Version(0, 1, 6)));
        assert_eq!(parse_version("V1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(parse_version("  v10.20.30  "), Some(Version(10, 20, 30)));
    }

    #[test]
    fn parse_version_ignores_a_suffix_after_the_patch_number() {
        assert_eq!(parse_version("0.1.6-beta.2"), Some(Version(0, 1, 6)));
        assert_eq!(parse_version("0.1.6+build5"), Some(Version(0, 1, 6)));
    }

    #[test]
    fn parse_version_rejects_anything_that_is_not_three_numbers() {
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("abc"), None);
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("1..3"), None);
        assert_eq!(parse_version("v"), None);
    }

    #[test]
    fn versions_compare_numerically_not_as_text() {
        assert!(Version(0, 1, 10) > Version(0, 1, 9));
        assert!(Version(1, 0, 0) > Version(0, 9, 9));
        assert!(Version(0, 2, 0) > Version(0, 1, 99));
        assert_eq!(Version(0, 1, 5), Version(0, 1, 5));
    }

    // ---- extract_min_version ----

    #[test]
    fn extract_min_version_finds_the_marker_line() {
        let body = "Fixes a download bug.\n\nmin-version: 0.1.6\n\nThanks!";
        assert_eq!(extract_min_version(body), Some(Version(0, 1, 6)));
    }

    #[test]
    fn extract_min_version_is_case_insensitive_and_accepts_a_v_prefix() {
        assert_eq!(extract_min_version("MIN-VERSION: v0.2.0"), Some(Version(0, 2, 0)));
        assert_eq!(extract_min_version("Min-Version:0.2.0"), Some(Version(0, 2, 0)));
    }

    #[test]
    fn extract_min_version_tolerates_markdown_list_and_quote_prefixes() {
        assert_eq!(extract_min_version("- min-version: 0.1.6"), Some(Version(0, 1, 6)));
        assert_eq!(extract_min_version("* min-version: 0.1.6"), Some(Version(0, 1, 6)));
        assert_eq!(extract_min_version("> min-version: 0.1.6"), Some(Version(0, 1, 6)));
    }

    #[test]
    fn extract_min_version_is_none_when_absent_or_unparsable() {
        assert_eq!(extract_min_version("Just release notes."), None);
        assert_eq!(extract_min_version("min-version: soon"), None);
        assert_eq!(extract_min_version(""), None);
    }

    #[test]
    fn extract_min_version_uses_only_the_first_matching_line() {
        // The first marker is unparsable, so it wins (and yields None) -- a later valid one is not used.
        let body = "min-version: soon\nmin-version: 0.1.6";
        assert_eq!(extract_min_version(body), None);
    }

    // ---- decide ----

    #[test]
    fn decide_is_none_when_latest_is_not_newer() {
        assert_eq!(decide(Version(0, 1, 5), Version(0, 1, 5), None), UpdateStatus::None);
        assert_eq!(decide(Version(0, 2, 0), Version(0, 1, 5), None), UpdateStatus::None);
    }

    #[test]
    fn decide_is_banner_when_latest_is_newer_and_no_minimum() {
        assert_eq!(decide(Version(0, 1, 5), Version(0, 1, 6), None), UpdateStatus::Banner);
    }

    #[test]
    fn decide_is_blocking_when_current_is_below_the_minimum() {
        let min = Some(Version(0, 1, 6));
        assert_eq!(decide(Version(0, 1, 5), Version(0, 1, 6), min), UpdateStatus::Blocking);
    }

    #[test]
    fn decide_is_banner_when_current_meets_the_minimum() {
        let min = Some(Version(0, 1, 5));
        assert_eq!(decide(Version(0, 1, 5), Version(0, 1, 6), min), UpdateStatus::Banner);
    }

    #[test]
    fn decide_ignores_a_minimum_higher_than_the_latest_release() {
        // A typo like min-version: 9.9.9 must never lock users out of an update that can't satisfy it.
        let min = Some(Version(9, 9, 9));
        assert_eq!(decide(Version(0, 1, 5), Version(0, 1, 6), min), UpdateStatus::Banner);
        assert_eq!(decide(Version(0, 1, 6), Version(0, 1, 6), min), UpdateStatus::None);
    }

    // ---- is_allowed_update_url ----

    #[test]
    fn only_urls_inside_our_github_repo_are_allowed() {
        assert!(is_allowed_update_url("https://github.com/vinaynasha/VsnapU_Downloader/releases/download/v0.1.6/a.msi"));
        assert!(is_allowed_update_url("https://github.com/vinaynasha/VsnapU_Downloader/releases/tag/v0.1.6"));
        assert!(!is_allowed_update_url("https://github.com/someone-else/VsnapU_Downloader/releases/tag/v1"));
        assert!(!is_allowed_update_url("http://github.com/vinaynasha/VsnapU_Downloader/releases/tag/v1"));
        assert!(!is_allowed_update_url("https://github.com.evil.com/vinaynasha/VsnapU_Downloader/x"));
        assert!(!is_allowed_update_url("https://evil.com/?https://github.com/vinaynasha/VsnapU_Downloader/"));
        assert!(!is_allowed_update_url(""));
    }

    // ---- pick_installer_url ----

    #[test]
    fn pick_installer_url_chooses_the_asset_for_the_os() {
        let release = release_json("v0.1.6", None);
        assert!(pick_installer_url(&release, "windows").ends_with("VSnapU.Downloader_x64_en-US.msi"));
        assert!(pick_installer_url(&release, "macos").ends_with("VSnapU.Downloader_x64.dmg"));
    }

    #[test]
    fn pick_installer_url_falls_back_to_the_release_page() {
        let release = release_json("v0.1.6", None);
        assert_eq!(pick_installer_url(&release, "linux"), release.html_url);

        let mut no_assets = release_json("v0.1.6", None);
        no_assets.assets.clear();
        assert_eq!(pick_installer_url(&no_assets, "windows"), no_assets.html_url);
    }

    #[test]
    fn pick_installer_url_matches_extensions_case_insensitively() {
        let mut release = release_json("v0.1.6", None);
        release.assets = vec![Asset {
            name: "SETUP.MSI".to_string(),
            browser_download_url: "https://github.com/vinaynasha/VsnapU_Downloader/releases/download/v0.1.6/SETUP.MSI".to_string(),
        }];
        assert!(pick_installer_url(&release, "windows").ends_with("SETUP.MSI"));
    }

    // ---- evaluate ----

    #[test]
    fn evaluate_reports_a_banner_for_a_newer_release() {
        let release = release_json("v0.1.6", Some("Nice release."));
        let result = evaluate("0.1.5", "windows", &release);
        assert_eq!(result.status, UpdateStatus::Banner);
        assert_eq!(result.latest_version, "0.1.6");
        assert!(result.installer_url.ends_with(".msi"));
    }

    #[test]
    fn evaluate_reports_nothing_when_up_to_date() {
        let release = release_json("v0.1.5", None);
        let result = evaluate("0.1.5", "windows", &release);
        assert_eq!(result, UpdateCheckResult::none());
    }

    #[test]
    fn evaluate_reports_blocking_for_a_critical_release() {
        let release = release_json("v0.1.6", Some("Security fix.\nmin-version: 0.1.6"));
        let result = evaluate("0.1.5", "macos", &release);
        assert_eq!(result.status, UpdateStatus::Blocking);
        assert!(result.installer_url.ends_with(".dmg"));
    }

    #[test]
    fn evaluate_ignores_a_minimum_above_the_latest_version() {
        let release = release_json("v0.1.6", Some("min-version: 9.9.9"));
        assert_eq!(evaluate("0.1.5", "windows", &release).status, UpdateStatus::Banner);
    }

    #[test]
    fn evaluate_is_silent_for_unparsable_versions() {
        let bad_tag = release_json("nightly", None);
        assert_eq!(evaluate("0.1.5", "windows", &bad_tag), UpdateCheckResult::none());

        let release = release_json("v0.1.6", None);
        assert_eq!(evaluate("dev-build", "windows", &release), UpdateCheckResult::none());
    }

    #[test]
    fn evaluate_never_hands_out_a_url_outside_our_repo() {
        let mut release = release_json("v0.1.6", None);
        release.assets[0].browser_download_url = "https://evil.example/a.msi".to_string();
        let result = evaluate("0.1.5", "windows", &release);
        // The bad asset URL is rejected, so it falls back to the (allowed) release page.
        assert_eq!(result.status, UpdateStatus::Banner);
        assert_eq!(result.installer_url, release.html_url);

        release.html_url = "https://evil.example/release".to_string();
        // Nothing safe left to open -> no update is offered at all.
        assert_eq!(evaluate("0.1.5", "windows", &release), UpdateCheckResult::none());
    }

    #[test]
    fn update_check_result_serialises_with_camel_case_keys() {
        let release = release_json("v0.1.6", None);
        let json = serde_json::to_value(evaluate("0.1.5", "windows", &release)).unwrap();
        assert_eq!(json["status"], "banner");
        assert_eq!(json["latestVersion"], "0.1.6");
        assert!(json["installerUrl"].as_str().unwrap().ends_with(".msi"));
    }
}
