use std::path::Path;

use serde::Deserialize;

use crate::instance::loader::GameVersion;
use crate::net::{download_file, HttpClient, NetError};
use crate::tui::progress::set_action;

const NEOFORGE_MAVEN_BASE: &str = "https://maven.neoforged.net/releases/net/neoforged/neoforge";
const NEOFORGE_API_BASE: &str =
    "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";

#[derive(Debug, Deserialize)]
struct NeoForgeMavenVersions {
    versions: Vec<String>,
}

/// Map a Minecraft game version like "1.21" or "1.20.4" to the NeoForge version
/// prefix (e.g. "21.0." or "20.4."). NeoForge versions encode the MC version as
/// `MAJOR.MINOR.` where MAJOR is the MC major-minor (20 for 1.20.x, 21 for 1.21.x)
/// and MINOR is the MC patch (0 if absent, e.g. "1.21" → "21.0.").
fn game_version_to_neoforge_prefix(game_version: &str) -> Option<String> {
    let parts: Vec<&str> = game_version.split('.').collect();
    match parts.as_slice() {
        // "1.21" → prefix "21.0."
        ["1", minor] => Some(format!("{}.0.", minor)),
        // "1.20.4" → prefix "20.4."
        ["1", minor, patch] => Some(format!("{}.{}.", minor, patch)),
        [major, minor] => Some(format!("{}.{}.", major, minor)),
        [major, minor, _patch] => Some(format!("{}.{}.", major, minor)),
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
            tracing::error!(
                "Cannot map game version '{}' to NeoForge version prefix",
                game_version
            );
            return Err(NetError::Parse(format!(
                "Invalid game version for NeoForge: {}",
                game_version
            )));
        }
    };

    let response = match client.inner().get(NEOFORGE_API_BASE).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("GET {} failed: {}", NEOFORGE_API_BASE, e);
            return Err(NetError::Http(e));
        }
    };

    if !response.status().is_success() {
        let status = response.status().as_u16();
        tracing::error!("HTTP {} for {}", status, NEOFORGE_API_BASE);
        return Err(NetError::StatusError {
            status,
            url: NEOFORGE_API_BASE.to_string(),
        });
    }

    let maven_versions: NeoForgeMavenVersions = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to parse NeoForge maven versions: {}", e);
            return Err(NetError::Http(e));
        }
    };

    let versions: Vec<String> = maven_versions
        .versions
        .into_iter()
        .filter(|v| v.starts_with(&prefix) && !v.contains("-beta") && !v.contains("-alpha"))
        .collect();

    Ok(versions)
}

pub async fn fetch_neoforge_game_versions(
    client: &HttpClient,
) -> Result<Vec<GameVersion>, NetError> {
    let response = match client.inner().get(NEOFORGE_API_BASE).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("GET {} failed: {}", NEOFORGE_API_BASE, e);
            return Err(NetError::Http(e));
        }
    };

    if !response.status().is_success() {
        let status = response.status().as_u16();
        tracing::error!("HTTP {} for {}", status, NEOFORGE_API_BASE);
        return Err(NetError::StatusError {
            status,
            url: NEOFORGE_API_BASE.to_string(),
        });
    }

    let maven: NeoForgeMavenVersions = match response.json().await {
        Ok(m) => m,
        Err(e) => {
            tracing::error!("Failed to parse NeoForge versions: {}", e);
            return Err(NetError::Http(e));
        }
    };

    let mut game_versions: Vec<String> = Vec::new();
    for version in &maven.versions {
        let parts: Vec<&str> = version.split('.').collect();
        if parts.len() >= 2 {
            if let (Ok(major), Ok(minor)) = (parts[0].parse::<u32>(), parts[1].parse::<u32>()) {
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

    match download_file(client, &url, dest, |downloaded, total| {
        crate::tui::progress::set_progress(downloaded, total);
    })
    .await
    {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::error!("Failed to download NeoForge installer from {}: {}", url, e);
            Err(e)
        }
    }
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
            tracing::error!("Failed to launch NeoForge installer: {}", e);
            return Err(NetError::Io(e));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        tracing::error!(
            "NeoForge installer failed (exit {:?}): {} {}",
            output.status.code(),
            stderr,
            stdout
        );
        return Err(NetError::Parse(format!(
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
