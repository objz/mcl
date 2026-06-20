// fabric mod loader: fetches loader metadata and downloads libraries
// from fabric's maven. structurally very similar to quilt (they forked).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::instance::loader::GameVersion;
use crate::net::{HttpClient, NetError, download_file};
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
    fetch_fabric_game_versions_from(client, FABRIC_META_BASE).await
}

// same as fetch_fabric_game_versions but lets tests point at a wiremock
// server. fabric's API is hierarchical, so we take the base URL and append
// the rest of the path inside.
pub async fn fetch_fabric_game_versions_from(
    client: &HttpClient,
    meta_base: &str,
) -> Result<Vec<GameVersion>, NetError> {
    let url = format!("{}/versions/game", meta_base);
    tracing::debug!("Fetching Fabric game versions from {}", url);
    let versions: Vec<FabricGameVersion> = client.get_json(&url).await?;
    tracing::debug!("Fetched {} Fabric game version(s)", versions.len());

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
    fetch_fabric_versions_from(client, FABRIC_META_BASE, game_version).await
}

pub async fn fetch_fabric_versions_from(
    client: &HttpClient,
    meta_base: &str,
    game_version: &str,
) -> Result<Vec<FabricLoaderVersion>, NetError> {
    let url = format!("{}/versions/loader/{}", meta_base, game_version);
    tracing::debug!("Fetching Fabric loader versions for {}", game_version);
    let versions: Vec<FabricLoaderVersion> = client.get_json(&url).await?;
    tracing::debug!(
        "Fetched {} Fabric loader version(s) for {}",
        versions.len(),
        game_version
    );
    Ok(versions)
}

pub async fn fetch_fabric_profile(
    client: &HttpClient,
    game_version: &str,
    loader_version: &str,
) -> Result<FabricProfile, NetError> {
    fetch_fabric_profile_from(client, FABRIC_META_BASE, game_version, loader_version).await
}

pub async fn fetch_fabric_profile_from(
    client: &HttpClient,
    meta_base: &str,
    game_version: &str,
    loader_version: &str,
) -> Result<FabricProfile, NetError> {
    let url = format!(
        "{}/versions/loader/{}/{}/profile/json",
        meta_base, game_version, loader_version
    );
    tracing::debug!(
        "Fetching Fabric profile for Minecraft {} loader {}",
        game_version,
        loader_version
    );
    client.get_json(&url).await
}

// like fetch_fabric_profile but also returns the raw response bytes so the
// caller can write the upstream JSON byte-for-byte to disk. used by the
// install path so we don't lose data (e.g. any future arguments field) by
// re-serializing through our narrow FabricProfile struct.
pub async fn fetch_fabric_profile_with_raw(
    client: &HttpClient,
    game_version: &str,
    loader_version: &str,
) -> Result<(FabricProfile, Vec<u8>), NetError> {
    fetch_fabric_profile_with_raw_from(client, FABRIC_META_BASE, game_version, loader_version).await
}

pub async fn fetch_fabric_profile_with_raw_from(
    client: &HttpClient,
    meta_base: &str,
    game_version: &str,
    loader_version: &str,
) -> Result<(FabricProfile, Vec<u8>), NetError> {
    let url = format!(
        "{}/versions/loader/{}/{}/profile/json",
        meta_base, game_version, loader_version
    );
    tracing::debug!(
        "Fetching raw Fabric profile for Minecraft {} loader {}",
        game_version,
        loader_version
    );
    client.get_json_with_raw(&url, "Fabric profile").await
}

// each fabric library entry has a maven coordinate and a base url.
// the coordinate gets resolved to a path and combined with the url to download.
pub async fn download_fabric_libraries(
    client: &HttpClient,
    profile: &FabricProfile,
    meta_dir: &Path,
) -> Result<(), NetError> {
    let libraries_dir = meta_dir.join("libraries");
    tracing::debug!(
        "Resolving {} Fabric libraries into {}",
        profile.libraries.len(),
        libraries_dir.display()
    );

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
        tracing::trace!("Fabric library destination: {}", dest.display());

        download_file(client, &download_url, &dest, |_, _| {}).await?;
    }

    tracing::debug!("Fabric library resolution complete for {}", profile.id);
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
