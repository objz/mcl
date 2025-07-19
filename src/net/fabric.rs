use std::path::Path;

use serde::Deserialize;

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
#[serde(rename_all = "camelCase")]
pub struct FabricProfile {
    pub id: String,
    pub main_class: String,
    pub libraries: Vec<FabricLibrary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FabricLibrary {
    pub name: String,
    pub url: String,
}

/// `net.fabricmc:fabric-loader:0.16.0` → `net/fabricmc/fabric-loader/0.16.0/fabric-loader-0.16.0.jar`
fn maven_coord_to_path(coord: &str) -> Option<String> {
    let parts: Vec<&str> = coord.split(':').collect();
    match parts.as_slice() {
        [group, artifact, version] => {
            let group_path = group.replace('.', "/");
            Some(format!(
                "{}/{}/{}/{}-{}.jar",
                group_path, artifact, version, artifact, version
            ))
        }
        _ => None,
    }
}

pub async fn fetch_fabric_versions(
    client: &HttpClient,
    game_version: &str,
) -> Result<Vec<FabricLoaderVersion>, NetError> {
    let url = format!("{}/versions/loader/{}", FABRIC_META_BASE, game_version);

    let response = match client.inner().get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Fabric meta GET {} failed: {}", url, e);
            return Err(NetError::Http(e));
        }
    };

    if !response.status().is_success() {
        let status = response.status().as_u16();
        tracing::error!("Fabric meta HTTP {} for {}", status, url);
        return Err(NetError::StatusError {
            status,
            url: url.clone(),
        });
    }

    let versions: Vec<FabricLoaderVersion> = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to parse Fabric loader versions: {}", e);
            return Err(NetError::Http(e));
        }
    };

    Ok(versions)
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

    let response = match client.inner().get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Fabric profile GET {} failed: {}", url, e);
            return Err(NetError::Http(e));
        }
    };

    if !response.status().is_success() {
        let status = response.status().as_u16();
        tracing::error!("Fabric profile HTTP {} for {}", status, url);
        return Err(NetError::StatusError {
            status,
            url: url.clone(),
        });
    }

    let profile: FabricProfile = match response.json().await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to parse Fabric profile: {}", e);
            return Err(NetError::Http(e));
        }
    };

    Ok(profile)
}

pub async fn download_fabric_libraries(
    client: &HttpClient,
    profile: &FabricProfile,
    instance_dir: &Path,
) -> Result<(), NetError> {
    let libraries_dir = instance_dir.join(".minecraft").join("libraries");

    for lib in &profile.libraries {
        let maven_path = match maven_coord_to_path(&lib.name) {
            Some(p) => p,
            None => {
                tracing::error!(
                    "Invalid Maven coordinate in Fabric profile: {}",
                    lib.name
                );
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

        match download_file(client, &download_url, &dest, |_, _| {}).await {
            Ok(()) => {}
            Err(e) => {
                tracing::error!(
                    "Failed to download Fabric library {}: {}",
                    lib.name, e
                );
                return Err(e);
            }
        }
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
            Err(e) => assert!(false, "fetch_fabric_versions failed: {}", e),
        }
    }

    #[test]
    fn test_maven_coord_to_path() {
        assert_eq!(
            maven_coord_to_path("net.fabricmc:fabric-loader:0.16.0"),
            Some("net/fabricmc/fabric-loader/0.16.0/fabric-loader-0.16.0.jar".to_string())
        );
        assert_eq!(
            maven_coord_to_path("org.ow2.asm:asm:9.6"),
            Some("org/ow2/asm/asm/9.6/asm-9.6.jar".to_string())
        );
        assert_eq!(maven_coord_to_path("invalid"), None);
        assert_eq!(maven_coord_to_path("only:two"), None);
    }
}
