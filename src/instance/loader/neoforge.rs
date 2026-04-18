// neoforge installation. same java installer dance as forge (they forked from it,
// after all), just with different URLs and version naming.

use std::path::Path;

use async_trait::async_trait;

use super::{GameVersion, ModLoaderInstaller};
use crate::instance::models::ModLoader;
use crate::net::{neoforge as neoforge_api, HttpClient, NetError};

pub struct NeoForgeInstaller;

#[async_trait]
impl ModLoaderInstaller for NeoForgeInstaller {
    fn loader_type(&self) -> ModLoader {
        ModLoader::NeoForge
    }

    async fn get_game_versions(&self, client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
        neoforge_api::fetch_neoforge_game_versions(client).await
    }

    async fn get_versions(
        &self,
        client: &HttpClient,
        game_version: &str,
    ) -> Result<Vec<String>, NetError> {
        neoforge_api::fetch_neoforge_versions(client, game_version).await
    }

    async fn install(
        &self,
        client: &HttpClient,
        _game_version: &str,
        loader_version: &str,
        instance_dir: &Path,
        meta_dir: &Path,
    ) -> Result<(), NetError> {
        let installer_jar = instance_dir
            .join(".minecraft")
            .join("neoforge-installer.jar");

        neoforge_api::download_neoforge_installer(client, loader_version, &installer_jar).await?;

        let java_path = crate::config::SETTINGS
            .paths
            .effective_java_path()
            .map(str::to_owned)
            .unwrap_or_else(crate::net::detect_java_path);
        if let Err(e) =
            neoforge_api::run_neoforge_installer(&installer_jar, instance_dir, &java_path).await
        {
            let _ = tokio::fs::remove_file(&installer_jar).await;
            return Err(e);
        }

        if let Err(e) = tokio::fs::remove_file(&installer_jar).await {
            tracing::warn!("Failed to remove NeoForge installer JAR: {}", e);
        }

        save_neoforge_profile(instance_dir, meta_dir, loader_version)?;

        Ok(())
    }
}

fn save_neoforge_profile(instance_dir: &Path, meta_dir: &Path, loader_version: &str) -> Result<(), NetError> {
    let version_dir_name = format!("neoforge-{loader_version}");
    let profile_filename = format!("neoforge-{loader_version}.json");
    super::save_installer_profile(instance_dir, meta_dir, &version_dir_name, &profile_filename)
}
