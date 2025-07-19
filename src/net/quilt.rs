use std::path::Path;

use serde::Deserialize;

use crate::net::{download_file, HttpClient, NetError};
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
#[serde(rename_all = "camelCase")]
pub struct QuiltProfile {
    pub id: String,
    pub main_class: String,
    pub libraries: Vec<QuiltLibrary>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuiltLibrary {
    pub name: String,
    pub url: String,
}

/// `org.quiltmc:quilt-loader:0.20.0` → `org/quiltmc/quilt-loader/0.20.0/quilt-loader-0.20.0.jar`
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

pub async fn fetch_quilt_versions(
    client: &HttpClient,
    game_version: &str,
) -> Result<Vec<QuiltLoaderVersion>, NetError> {
    let url = format!("{}/versions/loader/{}", QUILT_META_BASE, game_version);

    let response = match client.inner().get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Quilt meta GET {} failed: {}", url, e);
            return Err(NetError::Http(e));
        }
    };

    if !response.status().is_success() {
        let status = response.status().as_u16();
        tracing::error!("Quilt meta HTTP {} for {}", status, url);
        return Err(NetError::StatusError {
            status,
            url: url.clone(),
        });
    }

    let versions: Vec<QuiltLoaderVersion> = match response.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to parse Quilt loader versions: {}", e);
            return Err(NetError::Http(e));
        }
    };

    Ok(versions)
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

    let response = match client.inner().get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Quilt profile GET {} failed: {}", url, e);
            return Err(NetError::Http(e));
        }
    };

    if !response.status().is_success() {
        let status = response.status().as_u16();
        tracing::error!("Quilt profile HTTP {} for {}", status, url);
        return Err(NetError::StatusError {
            status,
            url: url.clone(),
        });
    }

    let profile: QuiltProfile = match response.json().await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to parse Quilt profile: {}", e);
            return Err(NetError::Http(e));
        }
    };

    Ok(profile)
}

pub async fn download_quilt_libraries(
    client: &HttpClient,
    profile: &QuiltProfile,
    instance_dir: &Path,
) -> Result<(), NetError> {
    let libraries_dir = instance_dir.join(".minecraft").join("libraries");

    for lib in &profile.libraries {
        let maven_path = match maven_coord_to_path(&lib.name) {
            Some(p) => p,
            None => {
                tracing::error!(
                    "Invalid Maven coordinate in Quilt profile: {}",
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
            tracing::debug!("Quilt library already exists: {}", lib.name);
            continue;
        }

        let base_url = lib.url.trim_end_matches('/');
        let download_url = format!("{}/{}", base_url, maven_path);

        set_sub_action(&lib.name);
        tracing::info!("Downloading Quilt library: {}", lib.name);

        match download_file(client, &download_url, &dest, |_, _| {}).await {
            Ok(()) => {}
            Err(e) => {
                tracing::error!(
                    "Failed to download Quilt library {}: {}",
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
        match fetch_quilt_versions(&client, "1.20.1").await {
            Ok(versions) => {
                assert!(!versions.is_empty(), "Should have Quilt versions for 1.20.1");
            }
            Err(e) => assert!(false, "fetch_quilt_versions failed: {}", e),
        }
    }

    #[test]
    fn test_maven_coord_to_path() {
        assert_eq!(
            maven_coord_to_path("org.quiltmc:quilt-loader:0.20.0"),
            Some("org/quiltmc/quilt-loader/0.20.0/quilt-loader-0.20.0.jar".to_string())
        );
        assert_eq!(maven_coord_to_path("invalid"), None);
        assert_eq!(maven_coord_to_path("only:two"), None);
    }
}
