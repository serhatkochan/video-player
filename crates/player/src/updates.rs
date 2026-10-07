use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::time::Duration;

pub const RELEASES_URL: &str = "https://github.com/serhatkochan/video-player/releases";

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
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
    let mut response = match response {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(error) => return Err(error).context("Cannot check GitHub Releases"),
    };
    let release: Release = response
        .body_mut()
        .with_config()
        .limit(1024 * 1024)
        .read_json()?;
    let version = release.tag_name.trim_start_matches('v').to_owned();
    if version_parts(&version).is_none() {
        bail!("GitHub returned an invalid release version");
    }
    Ok(Some(version))
}

pub fn newer_than(candidate: &str, current: &str) -> bool {
    match (version_parts(candidate), version_parts(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

fn version_parts(version: &str) -> Option<[u32; 3]> {
    if version.contains(['-', '+']) {
        return None;
    }
    let parts: Vec<u32> = version
        .trim_start_matches('v')
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    parts.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions_compare_numerically_and_reject_untrusted_values() {
        assert!(newer_than("0.10.0", "0.9.0"));
        assert!(!newer_than("0.1.0", "0.1.0"));
        assert!(!newer_than("../../setup.exe", "0.1.0"));
        assert!(!newer_than("0.2.0-beta", "0.1.0"));
    }
}
