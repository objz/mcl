// vanilla "installer". doesn't actually install anything since the launch
// process already handles downloading vanilla assets/libraries. this just
// exists so vanilla fits the same ModLoaderInstaller trait as everyone else.

use std::path::Path;

use async_trait::async_trait;

use super::{GameVersion, InstallError, ModLoaderInstaller};
use crate::instance::models::ModLoader;
use crate::net::{HttpClient, NetError, mojang};

pub struct VanillaInstaller;

#[async_trait]
impl ModLoaderInstaller for VanillaInstaller {
    fn loader_type(&self) -> ModLoader {
        ModLoader::Vanilla
    }

    async fn get_game_versions(&self, client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
        let manifest = mojang::fetch_version_manifest(client).await?;
        Ok(game_versions_from_manifest(manifest))
    }

    async fn get_versions(
        &self,
        _client: &HttpClient,
        _game_version: &str,
    ) -> Result<Vec<String>, NetError> {
        Ok(vec!["vanilla".to_owned()])
    }

    async fn install(
        &self,
        _client: &HttpClient,
        _game_version: &str,
        _loader_version: &str,
        _instance_dir: &Path,
        _meta_dir: &Path,
    ) -> Result<(), InstallError> {
        Ok(())
    }
}

fn game_versions_from_manifest(manifest: mojang::VersionManifest) -> Vec<GameVersion> {
    manifest
        .versions
        .into_iter()
        .map(|version| GameVersion {
            id: version.id,
            stable: version.version_type == "release",
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::mojang::{LatestVersions, VersionEntry, VersionManifest};

    #[test]
    fn manifest_types_map_to_stable_game_versions() {
        let manifest = VersionManifest {
            latest: LatestVersions {
                release: "1.21.1".to_owned(),
                snapshot: "24w01a".to_owned(),
            },
            versions: vec![
                VersionEntry {
                    id: "1.21.1".to_owned(),
                    version_type: "release".to_owned(),
                    url: String::new(),
                    sha1: String::new(),
                },
                VersionEntry {
                    id: "24w01a".to_owned(),
                    version_type: "snapshot".to_owned(),
                    url: String::new(),
                    sha1: String::new(),
                },
            ],
        };

        let versions = game_versions_from_manifest(manifest);

        assert_eq!(versions[0].id, "1.21.1");
        assert!(versions[0].stable);
        assert_eq!(versions[1].id, "24w01a");
        assert!(!versions[1].stable);
    }
}
