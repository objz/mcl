use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::instance::loader::GameVersion;
use crate::net::{download_file, HttpClient, NetError};
use crate::tui::progress::set_sub_action;

const FABRIC_META_BASE: &str = "https://meta.fabricmc.net/v2";

#[derive(Debug, Clone, Deserialize)]
pub struct FabricLoaderVersion {
    pub loader: FabricVersion,
    pub intermediary: FabricVersion,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FabricVersion {
    pub version: String,
    #[serde(default)]
    pub stable: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FabricGameVersion {
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FabricProfile {
    pub id: String,
    pub main_class: String,
    pub libraries: Vec<FabricLibrary>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FabricLibrary {
    pub name: String,
    pub url: String,
}

pub async fn fetch_fabric_game_versions(client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
    let url = format!("{}/versions/game", FABRIC_META_BASE);
    let versions: Vec<FabricGameVersion> = client.get_json(&url).await?;

    Ok(versions
        .into_iter()
        .map(|version| GameVersion {
            id: version.version,
            stable: version.stable,
        })
        .collect())
}

pub async fn fetch_fabric_versions(
    client: &HttpClient,
    game_version: &str,
) -> Result<Vec<FabricLoaderVersion>, NetError> {
    let url = format!("{}/versions/loader/{}", FABRIC_META_BASE, game_version);
    client.get_json(&url).await
}

pub async fn fetch_fabric_profile(
    client: &HttpClient,
    game_version: &str,
    loader_version: &str,
) -> Result<FabricProfile, NetError> {
    let url = format!(
        "{}/versions/loader/{}/{}/profile/json",
        FABRIC_META_BASE, game_version, loader_version
    );
    client.get_json(&url).await
}

pub async fn download_fabric_libraries(
    client: &HttpClient,
    profile: &FabricProfile,
    meta_dir: &Path,
) -> Result<(), NetError> {
    let libraries_dir = meta_dir.join("libraries");

    for lib in &profile.libraries {
        let maven_path = match crate::net::maven_coord_to_path(&lib.name) {
            Some(p) => p,
            None => {
                return Err(NetError::Parse(format!(
                    "Invalid Maven coordinate: {}",
                    lib.name
                )));
            }
        };

        let dest = libraries_dir.join(&maven_path);

        if dest.exists() {
            tracing::debug!("Fabric library already exists: {}", lib.name);
            continue;
        }

        let base_url = lib.url.trim_end_matches('/');
        let download_url = format!("{}/{}", base_url, maven_path);

        set_sub_action(&lib.name);
        tracing::info!("Downloading Fabric library: {}", lib.name);

        download_file(client, &download_url, &dest, |_, _| {}).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::HttpClient;

    #[tokio::test]
    #[ignore = "hits live Fabric API"]
    async fn test_fetch_versions() {
        let client = HttpClient::new();
        match fetch_fabric_versions(&client, "1.20.1").await {
            Ok(versions) => {
                assert!(
                    !versions.is_empty(),
                    "Should have Fabric versions for 1.20.1"
                );
                assert!(
                    versions[0].loader.version.contains('.'),
                    "Version should be semver-like"
                );
            }
            Err(e) => panic!("fetch_fabric_versions failed: {}", e),
        }
    }

    #[tokio::test]
    #[ignore = "hits live Fabric API"]
    async fn test_fetch_game_versions() {
        let client = HttpClient::new();
        match fetch_fabric_game_versions(&client).await {
            Ok(versions) => {
                assert!(!versions.is_empty(), "Should have Fabric game versions");
                assert!(versions.iter().any(|version| version.id == "1.20.1"));
            }
            Err(e) => panic!("fetch_fabric_game_versions failed: {}", e),
        }
    }

}
