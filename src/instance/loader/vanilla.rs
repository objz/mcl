// vanilla "installer". doesn't actually install anything since the launch
// process already handles downloading vanilla assets/libraries. this just
// exists so vanilla fits the same ModLoaderInstaller trait as everyone else.

use std::path::Path;

use async_trait::async_trait;

use super::{GameVersion, ModLoaderInstaller};
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
        Ok(manifest
            .versions
            .into_iter()
            .map(|v| GameVersion {
                id: v.id,
                stable: v.version_type == "release",
            })
            .collect())
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
    ) -> Result<(), NetError> {
        Ok(())
    }
}
