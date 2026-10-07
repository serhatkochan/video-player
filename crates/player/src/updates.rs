use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;

pub const RELEASES_URL: &str = "https://github.com/serhatkochan/video-player/releases";

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    state: String,
    size: u64,
}

pub fn latest_version() -> Result<Option<String>> {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .build();
    let agent: ureq::Agent = config.into();
    let response = agent
        .get("https://api.github.com/repos/serhatkochan/video-player/releases/latest")
        .header(
            "User-Agent",
            concat!("Video-Player/", env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", "application/vnd.github+json")
        .call();
    match response {
        Ok(mut response) => {
            let release: Release = response
                .body_mut()
                .with_config()
                .limit(1024 * 1024)
                .read_json()?;
            if let Some(version) = select_version(Some(&release), &[]) {
                return Ok(Some(version));
            }
        }
        Err(ureq::Error::StatusCode(404)) => {}
        Err(error) => return Err(error).context("Cannot check GitHub Releases"),
    }
    let mut response = match agent
        .get("https://api.github.com/repos/serhatkochan/video-player/releases?per_page=100")
        .header(
            "User-Agent",
            concat!("Video-Player/", env!("CARGO_PKG_VERSION")),
        )
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(error) => return Err(error).context("Cannot check GitHub Releases"),
    };
    let releases: Vec<Release> = response
        .body_mut()
        .with_config()
        .limit(4 * 1024 * 1024)
        .read_json()?;
    Ok(select_version(None, &releases))
}

fn select_version(latest: Option<&Release>, releases: &[Release]) -> Option<String> {
    latest
        .filter(|release| !release.prerelease)
        .and_then(application_version)
        .or_else(|| releases.iter().find_map(application_version))
}

fn application_version(release: &Release) -> Option<String> {
    if release.draft || release.published_at.as_deref().is_none_or(str::is_empty) {
        return None;
    }
    version_parts(&release.tag_name)?;
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    let installer = format!("Video-Player-{version}-windows-x64-setup.exe");
    release
        .assets
        .iter()
        .any(|asset| asset.name == installer && asset.state == "uploaded" && asset.size > 0)
        .then(|| version.to_owned())
}

pub fn newer_than(candidate: &str, current: &str) -> bool {
    match (version_parts(candidate), version_parts(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

fn version_parts(version: &str) -> Option<[u32; 3]> {
    let mut parts = version.strip_prefix('v').unwrap_or(version).split('.');
    let mut result = [0; 3];
    for number in &mut result {
        let part = parts.next()?;
        if part.is_empty()
            || !part.bytes().all(|byte| byte.is_ascii_digit())
            || (part.len() > 1 && part.starts_with('0'))
        {
            return None;
        }
        *number = part.parse().ok()?;
    }
    parts.next().is_none().then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_compare_numerically_and_reject_untrusted_values() {
        assert!(newer_than("0.10.0", "0.9.0"));
        assert!(newer_than("v0.2.0", "0.1.0"));
        assert!(!newer_than("0.1.0", "0.1.0"));
        for invalid in [
            "../../setup.exe",
            "0.2.0-beta",
            "0.2.0+build",
            "vv0.2.0",
            " 0.2.0",
            "0.02.0",
            "0.2",
            "0.2.0.1",
            "4294967296.0.0",
        ] {
            assert!(!newer_than(invalid, "0.1.0"), "{invalid}");
        }
    }

    #[test]
    fn preview_installer_is_available_without_a_latest_stable_release() {
        let releases: Vec<Release> = serde_json::from_str(
            r#"[{"tag_name":"v0.1.0","draft":false,"prerelease":true,
            "published_at":"2026-10-07T12:00:00Z","assets":[
                {"name":"Video-Player-0.1.0-windows-x64-setup.exe","state":"uploaded","size":1234}
            ]}]"#,
        )
        .unwrap();
        assert_eq!(select_version(None, &releases).as_deref(), Some("0.1.0"));
    }

    #[test]
    fn fallback_skips_runtime_components_drafts_and_missing_installers() {
        let releases: Vec<Release> = serde_json::from_str(
            r#"[
            {"tag_name":"runtime-20261007","draft":false,"prerelease":true,
                "published_at":"2026-10-07T12:00:00Z","assets":[
                    {"name":"runtime.zip","state":"uploaded","size":1234}]},
            {"tag_name":"v0.4.0","draft":true,"prerelease":false,
                "published_at":null,"assets":[
                    {"name":"Video-Player-0.4.0-windows-x64-setup.exe","state":"uploaded","size":1234}]},
            {"tag_name":"v0.3.0","draft":false,"prerelease":false,
                "published_at":"2026-10-07T12:00:00Z","assets":[
                    {"name":"Video-Player-0.3.0-windows-x64-setup.exe.sha256","state":"uploaded","size":1234}]},
            {"tag_name":"v0.2.0","draft":false,"prerelease":false,
                "published_at":"2026-10-07T12:00:00Z","assets":[
                    {"name":"Video-Player-0.1.0-windows-x64-setup.exe","state":"uploaded","size":1234}]},
            {"tag_name":"v0.1.0","draft":false,"prerelease":true,
                "published_at":"2026-10-07T12:00:00Z","assets":[
                    {"name":"Video-Player-0.1.0-windows-x64-setup.exe","state":"uploaded","size":1234}]}
            ]"#,
        )
        .unwrap();
        assert_eq!(select_version(None, &releases).as_deref(), Some("0.1.0"));
        assert_eq!(select_version(None, &releases[..4]), None);
        assert_eq!(
            select_version(Some(&releases[0]), &releases),
            select_version(None, &releases)
        );
    }

    #[test]
    fn valid_latest_stable_release_takes_priority_over_preview_list() {
        let stable: Release = serde_json::from_str(
            r#"{"tag_name":"v0.1.1","draft":false,"prerelease":false,
            "published_at":"2026-10-07T12:00:00Z","assets":[
                {"name":"Video-Player-0.1.1-windows-x64-setup.exe","state":"uploaded","size":1234}
            ]}"#,
        )
        .unwrap();
        let preview: Vec<Release> = serde_json::from_str(
            r#"[{"tag_name":"v0.2.0","draft":false,"prerelease":true,
            "published_at":"2026-10-07T12:00:00Z","assets":[
                {"name":"Video-Player-0.2.0-windows-x64-setup.exe","state":"uploaded","size":1234}
            ]}]"#,
        )
        .unwrap();
        assert_eq!(
            select_version(Some(&stable), &preview).as_deref(),
            Some("0.1.1")
        );
        assert_eq!(select_version(Some(&stable), &[]).as_deref(), Some("0.1.1"));
    }

    #[test]
    fn invalid_versions_and_unfinished_assets_cannot_be_updates() {
        let fixture = r#"{"tag_name":"v0.1.0","draft":false,"prerelease":true,
            "published_at":"2026-10-07T12:00:00Z","assets":[
                {"name":"Video-Player-0.1.0-windows-x64-setup.exe","state":"uploaded","size":1234}
            ]}"#;
        for (from, to) in [
            ("v0.1.0", "vv0.1.0"),
            ("v0.1.0", "v0.1.0-preview"),
            ("\"draft\":false", "\"draft\":true"),
            ("\"state\":\"uploaded\"", "\"state\":\"starter\""),
            ("\"size\":1234", "\"size\":0"),
            (
                "\"published_at\":\"2026-10-07T12:00:00Z\"",
                "\"published_at\":null",
            ),
        ] {
            let release: Release = serde_json::from_str(&fixture.replace(from, to)).unwrap();
            assert_eq!(application_version(&release), None, "{to}");
        }
    }
}
