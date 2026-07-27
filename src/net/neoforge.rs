// neoforge: the forge fork that broke away after 1.20.1.
// uses a different versioning scheme where neoforge versions map to
// minecraft versions by dropping the "1." prefix (e.g. MC 1.21 = NF 21.x).
// like forge, installation requires running their installer jar.

use std::path::Path;

use serde::Deserialize;

use crate::feedback::progress::set_action;
use crate::instance::loader::GameVersion;
use crate::net::{HttpClient, NetError, download_file};

const NEOFORGE_MAVEN_BASE: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const NEOFORGE_API_BASE: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

#[derive(Debug, Deserialize)]
struct NeoForgeMavenVersions {
    versions: Vec<String>,
}

// neoforge historically dropped the "1." from minecraft versions in
// their scheme, so "1.20.4" becomes prefix "20.4." and "1.21" becomes
// "21.0.". newer minecraft versions such as "26.1.2" keep their full
// version at the start of the neoforge coordinate: "26.1.2.76".
fn game_version_to_neoforge_prefix(game_version: &str) -> Option<String> {
    let parts: Vec<&str> = game_version.split('.').collect();
    match parts.as_slice() {
        // "1.21" → prefix "21.0."
        ["1", minor] => Some(format!("{}.0.", minor)),
        // "1.20.4" → prefix "20.4."
        ["1", minor, patch] => Some(format!("{}.{}.", minor, patch)),
        // "26.1.2" → prefix "26.1.2."
        [major, minor, patch] if leading_u32(major).is_some_and(|major| major >= 26) => {
            Some(format!("{major}.{minor}.{patch}."))
        }
        // "26.1" → prefix "26.1."
        [major, minor] if leading_u32(major).is_some_and(|major| major >= 26) => {
            Some(format!("{major}.{minor}."))
        }
        _ => None,
    }
}

fn leading_u32(raw: &str) -> Option<u32> {
    let digits: String = raw.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

fn neoforge_version_to_game_version(version: &str) -> Option<String> {
    let parts: Vec<&str> = version.split('.').collect();
    let major = leading_u32(parts.first()?)?;

    if major >= 26 {
        let minor = leading_u32(parts.get(1)?)?;
        let patch = leading_u32(parts.get(2)?)?;
        return Some(format!("{major}.{minor}.{patch}"));
    }

    let minor = leading_u32(parts.get(1)?)?;
    Some(if minor == 0 {
        format!("1.{major}")
    } else {
        format!("1.{major}.{minor}")
    })
}

pub async fn fetch_neoforge_versions(
    client: &HttpClient,
    game_version: &str,
) -> Result<Vec<String>, NetError> {
    fetch_neoforge_versions_from(client, NEOFORGE_API_BASE, game_version).await
}

// same as fetch_neoforge_versions but lets tests point at a wiremock server.
pub async fn fetch_neoforge_versions_from(
    client: &HttpClient,
    api_url: &str,
    game_version: &str,
) -> Result<Vec<String>, NetError> {
    let prefix = match game_version_to_neoforge_prefix(game_version) {
        Some(p) => p,
        None => {
            return Err(NetError::Parse(format!(
                "Invalid game version for NeoForge: {}",
                game_version
            )));
        }
    };

    let maven_versions: NeoForgeMavenVersions = client.get_json(api_url).await?;

    let versions: Vec<String> = maven_versions
        .versions
        .into_iter()
        .filter(|v| v.starts_with(&prefix) && !v.contains("-beta") && !v.contains("-alpha"))
        .collect();

    tracing::debug!(
        "Resolved {} NeoForge version(s) for Minecraft {} with prefix {}",
        versions.len(),
        game_version,
        prefix
    );
    Ok(versions)
}

// reverse-engineers minecraft versions from neoforge version numbers.
// e.g. neoforge "21.0.x" means MC 1.21, "20.4.x" means MC 1.20.4
pub async fn fetch_neoforge_game_versions(
    client: &HttpClient,
) -> Result<Vec<GameVersion>, NetError> {
    fetch_neoforge_game_versions_from(client, NEOFORGE_API_BASE).await
}

pub async fn fetch_neoforge_game_versions_from(
    client: &HttpClient,
    api_url: &str,
) -> Result<Vec<GameVersion>, NetError> {
    let maven: NeoForgeMavenVersions = client.get_json(api_url).await?;

    let mut game_versions: Vec<String> = Vec::new();
    for version in &maven.versions {
        if let Some(mc_version) = neoforge_version_to_game_version(version)
            && !game_versions.contains(&mc_version)
        {
            game_versions.push(mc_version);
        }
    }
    game_versions.reverse();
    tracing::debug!("Resolved {} NeoForge game version(s)", game_versions.len());

    Ok(game_versions
        .into_iter()
        .map(|version| GameVersion {
            id: version,
            stable: true,
        })
        .collect())
}

pub async fn download_neoforge_installer(
    client: &HttpClient,
    neoforge_version: &str,
    dest: &Path,
) -> Result<(), NetError> {
    let url = format!(
        "{}/{}/neoforge-{}-installer.jar",
        NEOFORGE_MAVEN_BASE, neoforge_version, neoforge_version
    );

    set_action(format!("Downloading NeoForge {}...", neoforge_version));
    tracing::info!(
        "Downloading NeoForge installer {} to {}",
        neoforge_version,
        dest.display()
    );

    download_file(client, &url, dest, |downloaded, total| {
        crate::feedback::progress::set_progress(downloaded, total);
    })
    .await
}

#[cfg(test)]
#[path = "tests/neoforge.rs"]
mod tests;
