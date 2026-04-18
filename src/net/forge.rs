// forge mod loader: version discovery via promotions API, then download
// and run the installer jar. forge doesn't have a nice metadata API like
// fabric/quilt, so it shells out to java to run their installer.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::instance::loader::GameVersion;
use crate::net::{HttpClient, NetError, download_file};
use crate::tui::progress::set_action;

const FORGE_PROMOTIONS_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const FORGE_MAVEN_BASE: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";

#[derive(Debug, Deserialize)]
struct ForgePromotions {
    promos: HashMap<String, String>,
}

// forge promotions use keys like "1.20.1-recommended", "1.20.1-latest"
// so this filters by game version prefix and extracts the forge version values
pub async fn fetch_forge_versions(
    client: &HttpClient,
    game_version: &str,
) -> Result<Vec<String>, NetError> {
    let promotions: ForgePromotions = client.get_json(FORGE_PROMOTIONS_URL).await?;

    let prefix = format!("{}-", game_version);
    let mut versions: Vec<String> = promotions
        .promos
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .map(|(_, value)| value.clone())
        .collect();

    versions.sort();
    versions.dedup();
    Ok(versions)
}

// extracts unique game versions from the promotion keys by splitting off
// the "-recommended"/"-latest" suffix
pub async fn fetch_forge_game_versions(client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
    let promos: ForgePromotions = client.get_json(FORGE_PROMOTIONS_URL).await?;

    let mut game_versions: Vec<String> = promos
        .promos
        .keys()
        .filter_map(|key| key.rsplit_once('-').map(|(version, _)| version.to_string()))
        .collect();
    game_versions.sort();
    game_versions.dedup();
    game_versions.reverse();

    Ok(game_versions
        .into_iter()
        .map(|version| GameVersion {
            id: version,
            stable: true,
        })
        .collect())
}

pub async fn download_forge_installer(
    client: &HttpClient,
    game_version: &str,
    forge_version: &str,
    dest: &Path,
) -> Result<(), NetError> {
    let url = format!(
        "{}/{game_version}-{forge_version}/forge-{game_version}-{forge_version}-installer.jar",
        FORGE_MAVEN_BASE,
        game_version = game_version,
        forge_version = forge_version,
    );

    set_action(format!(
        "Downloading Forge {}-{}...",
        game_version, forge_version
    ));

    download_file(client, &url, dest, |downloaded, total| {
        crate::tui::progress::set_progress(downloaded, total);
    })
    .await
}

pub async fn run_forge_installer(
    installer_path: &Path,
    instance_dir: &Path,
    java_path: &str,
) -> Result<(), NetError> {
    use tokio::process::Command;

    set_action("Running Forge installer...");

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
            "Forge installer exited with {:?}",
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
    #[ignore = "hits live Forge API"]
    async fn test_fetch_versions() {
        let client = HttpClient::new();
        match fetch_forge_versions(&client, "1.20.1").await {
            Ok(versions) => {
                assert!(
                    !versions.is_empty(),
                    "Should have Forge versions for 1.20.1"
                );
            }
            Err(e) => panic!("fetch_forge_versions failed: {}", e),
        }
    }

    #[tokio::test]
    #[ignore = "hits live Forge API"]
    async fn test_fetch_game_versions() {
        let client = HttpClient::new();
        match fetch_forge_game_versions(&client).await {
            Ok(versions) => {
                assert!(!versions.is_empty(), "Should have Forge game versions");
                assert!(versions.iter().any(|version| version.id == "1.20.1"));
            }
            Err(e) => panic!("fetch_forge_game_versions failed: {}", e),
        }
    }
}
