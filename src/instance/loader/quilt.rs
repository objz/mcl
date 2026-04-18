// quilt installation. same clean approach as fabric (profile json + library
// downloads). they're basically fabric's cooler sibling.

use std::path::Path;

use async_trait::async_trait;

use super::{GameVersion, ModLoaderInstaller};
use crate::instance::models::ModLoader;
use crate::net::{quilt as quilt_api, HttpClient, NetError};

pub struct QuiltInstaller;

#[async_trait]
impl ModLoaderInstaller for QuiltInstaller {
    fn loader_type(&self) -> ModLoader {
        ModLoader::Quilt
    }

    async fn get_game_versions(&self, client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
        quilt_api::fetch_quilt_game_versions(client).await
    }

    async fn get_versions(
        &self,
        client: &HttpClient,
        game_version: &str,
    ) -> Result<Vec<String>, NetError> {
        let loader_versions = quilt_api::fetch_quilt_versions(client, game_version).await?;
        Ok(loader_versions
            .into_iter()
            .map(|lv| lv.loader.version)
            .collect())
    }

    async fn install(
        &self,
        client: &HttpClient,
        game_version: &str,
        loader_version: &str,
        _instance_dir: &Path,
        meta_dir: &Path,
    ) -> Result<(), NetError> {
        let profile =
            quilt_api::fetch_quilt_profile(client, game_version, loader_version).await?;
        quilt_api::download_quilt_libraries(client, &profile, meta_dir).await?;

        super::save_profile_json(
            meta_dir,
            &format!("quilt-{game_version}-{loader_version}.json"),
            &profile,
        )?;

        Ok(())
    }
}
