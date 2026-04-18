// quilt mod loader: fabric fork with a nearly identical metadata API.
// if you're getting deja vu reading this after fabric.rs, that's why.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::instance::loader::GameVersion;
use crate::net::{HttpClient, NetError, download_file};
use crate::tui::progress::set_sub_action;

const QUILT_META_BASE: &str = "https://meta.quiltmc.org/v3";

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltLoaderVersion {
    pub loader: QuiltVersion,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltVersion {
    pub version: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltGameVersion {
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuiltProfile {
    pub id: String,
    pub main_class: String,
    pub libraries: Vec<QuiltLibrary>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QuiltLibrary {
    pub name: String,
    pub url: String,
}

pub async fn fetch_quilt_game_versions(client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
    let url = format!("{}/versions/game", QUILT_META_BASE);
    let versions: Vec<QuiltGameVersion> = client.get_json(&url).await?;

    Ok(versions
        .into_iter()
        .map(|version| GameVersion {
            id: version.version,
            stable: version.stable,
        })
        .collect())
}

pub async fn fetch_quilt_versions(
    client: &HttpClient,
    game_version: &str,
) -> Result<Vec<QuiltLoaderVersion>, NetError> {
    let url = format!("{}/versions/loader/{}", QUILT_META_BASE, game_version);
    client.get_json(&url).await
}

pub async fn fetch_quilt_profile(
    client: &HttpClient,
    game_version: &str,
    loader_version: &str,
) -> Result<QuiltProfile, NetError> {
    let url = format!(
        "{}/versions/loader/{}/{}/profile/json",
        QUILT_META_BASE, game_version, loader_version
    );
    client.get_json(&url).await
}

pub async fn download_quilt_libraries(
    client: &HttpClient,
    profile: &QuiltProfile,
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
            tracing::debug!("Quilt library already exists: {}", lib.name);
            continue;
        }

        let base_url = lib.url.trim_end_matches('/');
        let download_url = format!("{}/{}", base_url, maven_path);

        set_sub_action(&lib.name);
        tracing::info!("Downloading Quilt library: {}", lib.name);

        download_file(client, &download_url, &dest, |_, _| {}).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::HttpClient;

    #[tokio::test]
    #[ignore = "hits live Quilt API"]
    async fn test_fetch_versions() {
        let client = HttpClient::new();
        match fetch_quilt_versions(&client, "1.20.1").await {
            Ok(versions) => {
                assert!(
                    !versions.is_empty(),
                    "Should have Quilt versions for 1.20.1"
                );
            }
            Err(e) => panic!("fetch_quilt_versions failed: {}", e),
        }
    }

    #[tokio::test]
    #[ignore = "hits live Quilt API"]
    async fn test_fetch_game_versions() {
        let client = HttpClient::new();
        match fetch_quilt_game_versions(&client).await {
            Ok(versions) => {
                assert!(!versions.is_empty(), "Should have Quilt game versions");
                assert!(versions.iter().any(|version| version.id == "1.20.1"));
            }
            Err(e) => panic!("fetch_quilt_game_versions failed: {}", e),
        }
    }
}
