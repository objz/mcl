// quilt installation. same clean approach as fabric (profile json + library
// downloads). they're basically fabric's cooler sibling.

use std::path::Path;

use async_trait::async_trait;

use super::{GameVersion, InstallError, InstallerError, ModLoaderInstaller};
use crate::instance::models::ModLoader;
use crate::net::{HttpClient, NetError, quilt as quilt_api};

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
        tracing::debug!(
            "Fetching Quilt loader versions for Minecraft {}",
            game_version
        );
        let loader_versions = quilt_api::fetch_quilt_versions(client, game_version).await?;
        tracing::debug!(
            "Fetched {} Quilt loader version(s) for Minecraft {}",
            loader_versions.len(),
            game_version
        );
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
    ) -> Result<(), InstallError> {
        tracing::info!(
            "Installing Quilt {} for Minecraft {}",
            loader_version,
            game_version
        );
        let (profile, raw_bytes) =
            quilt_api::fetch_quilt_profile_with_raw(client, game_version, loader_version).await?;
        tracing::debug!(
            "Fetched Quilt profile {} with {} libraries",
            profile.id,
            profile.libraries.len()
        );
        quilt_api::download_quilt_libraries(client, &profile, meta_dir).await?;
        super::save_profile_bytes(
            meta_dir,
            &format!("quilt-{game_version}-{loader_version}.json"),
            &raw_bytes,
        )
        .map_err(InstallerError::from)?;
        tracing::debug!(
            "Saved Quilt profile for Minecraft {} loader {}",
            game_version,
            loader_version
        );
        Ok(())
    }
}
