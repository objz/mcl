// neoforge: the forge fork that broke away after 1.20.1.
// uses a different versioning scheme where neoforge versions map to
// minecraft versions by dropping the "1." prefix (e.g. MC 1.21 = NF 21.x).
// like forge, installation requires running their installer jar.

use std::path::Path;

use serde::Deserialize;

use crate::instance::loader::GameVersion;
use crate::net::{HttpClient, NetError, download_file};
use crate::tui::progress::set_action;

const NEOFORGE_MAVEN_BASE: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const NEOFORGE_API_BASE: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

#[derive(Debug, Deserialize)]
struct NeoForgeMavenVersions {
    versions: Vec<String>,
}

// neoforge dropped the "1." from minecraft versions in their scheme,
// so "1.20.4" becomes prefix "20.4." and "1.21" becomes "21.0."
fn game_version_to_neoforge_prefix(game_version: &str) -> Option<String> {
    let parts: Vec<&str> = game_version.split('.').collect();
    match parts.as_slice() {
        // "1.21" → prefix "21.0."
        ["1", minor] => Some(format!("{}.0.", minor)),
        // "1.20.4" → prefix "20.4."
        ["1", minor, patch] => Some(format!("{}.{}.", minor, patch)),
        _ => None,
    }
}

pub async fn fetch_neoforge_versions(
    client: &HttpClient,
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

    let maven_versions: NeoForgeMavenVersions = client.get_json(NEOFORGE_API_BASE).await?;

    let versions: Vec<String> = maven_versions
        .versions
        .into_iter()
        .filter(|v| v.starts_with(&prefix) && !v.contains("-beta") && !v.contains("-alpha"))
        .collect();

    Ok(versions)
}

// reverse-engineers minecraft versions from neoforge version numbers.
// e.g. neoforge "21.0.x" means MC 1.21, "20.4.x" means MC 1.20.4
pub async fn fetch_neoforge_game_versions(
    client: &HttpClient,
) -> Result<Vec<GameVersion>, NetError> {
    let maven: NeoForgeMavenVersions = client.get_json(NEOFORGE_API_BASE).await?;

    let mut game_versions: Vec<String> = Vec::new();
    for version in &maven.versions {
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() >= 2
            && let (Ok(major), Ok(minor)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>())
        {
            let mc_version = if minor == 0 {
                format!("1.{}", major)
            } else {
                format!("1.{}.{}", major, minor)
            };

            if !game_versions.contains(&mc_version) {
                game_versions.push(mc_version);
            }
        }
    }
    game_versions.reverse();

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

    download_file(client, &url, dest, |downloaded, total| {
        crate::tui::progress::set_progress(downloaded, total);
    })
    .await
}

pub async fn run_neoforge_installer(
    installer_path: &Path,
    instance_dir: &Path,
    java_path: &str,
) -> Result<(), NetError> {
    use tokio::process::Command;

    set_action("Running NeoForge installer...");

    let output = match Command::new(java_path)
        .arg("-jar")
        .arg(installer_path)
        .arg("--installClient")
        .current_dir(instance_dir.join(".minecraft"))
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            return Err(NetError::Io(e));
        }
    };

    if !output.status.success() {
        return Err(NetError::InstallerFailed(format!(
            "NeoForge installer exited with {:?}",
            output.status.code()
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::HttpClient;

    #[tokio::test]
    #[ignore = "hits live NeoForge API"]
    async fn test_fetch_versions() {
        let client = HttpClient::new();
        match fetch_neoforge_versions(&client, "1.21").await {
            Ok(versions) => {
                assert!(
                    !versions.is_empty(),
                    "Should have NeoForge versions for 1.21"
                );
            }
            Err(e) => panic!("fetch_neoforge_versions failed: {}", e),
        }
    }

    #[tokio::test]
    #[ignore = "hits live NeoForge API"]
    async fn test_fetch_game_versions() {
        let client = HttpClient::new();
        match fetch_neoforge_game_versions(&client).await {
            Ok(versions) => {
                assert!(!versions.is_empty(), "Should have NeoForge game versions");
                assert!(versions.iter().any(|version| version.id == "1.21"));
            }
            Err(e) => panic!("fetch_neoforge_game_versions failed: {}", e),
        }
    }

    #[test]
    fn test_game_version_to_neoforge_prefix() {
        assert_eq!(
            game_version_to_neoforge_prefix("1.21"),
            Some("21.0.".to_string())
        );
        assert_eq!(
            game_version_to_neoforge_prefix("1.20.4"),
            Some("20.4.".to_string())
        );
        assert_eq!(
            game_version_to_neoforge_prefix("1.21.1"),
            Some("21.1.".to_string())
        );
        assert_eq!(game_version_to_neoforge_prefix("invalid"), None);
    }
}
