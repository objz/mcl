// forge installation. unlike fabric/quilt, forge ships a java-based installer
// that has to be downloaded and executed. yes, a jvm is needed just to install
// the thing that runs on a jvm. the installer jar gets cleaned up afterward.

use std::path::Path;

use async_trait::async_trait;

use super::{GameVersion, ModLoaderInstaller};
use crate::instance::models::ModLoader;
use crate::net::{forge as forge_api, HttpClient, NetError};

pub struct ForgeInstaller;

#[async_trait]
impl ModLoaderInstaller for ForgeInstaller {
    fn loader_type(&self) -> ModLoader {
        ModLoader::Forge
    }

    async fn get_game_versions(&self, client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
        forge_api::fetch_forge_game_versions(client).await
    }

    async fn get_versions(
        &self,
        client: &HttpClient,
        game_version: &str,
    ) -> Result<Vec<String>, NetError> {
        forge_api::fetch_forge_versions(client, game_version).await
    }

    async fn install(
        &self,
        client: &HttpClient,
        game_version: &str,
        loader_version: &str,
        instance_dir: &Path,
        meta_dir: &Path,
    ) -> Result<(), NetError> {
        let installer_jar = instance_dir.join(".minecraft").join("forge-installer.jar");

        forge_api::download_forge_installer(client, game_version, loader_version, &installer_jar)
            .await?;

        // use configured java or try to find one on PATH
        let java_path = crate::config::SETTINGS
            .paths
            .effective_java_path()
            .map(str::to_owned)
            .unwrap_or_else(crate::net::detect_java_path);
        if let Err(e) =
            forge_api::run_forge_installer(&installer_jar, instance_dir, &java_path).await
        {
            // still clean up even if installation failed
            let _ = tokio::fs::remove_file(&installer_jar).await;
            return Err(e);
        }

        if let Err(e) = tokio::fs::remove_file(&installer_jar).await {
            tracing::warn!("Failed to remove Forge installer JAR: {}", e);
        }

        // extract the profile from what the installer just wrote to disk
        save_forge_profile(instance_dir, meta_dir, game_version, loader_version)?;

        Ok(())
    }
}

fn save_forge_profile(
    instance_dir: &Path,
    meta_dir: &Path,
    game_version: &str,
    loader_version: &str,
) -> Result<(), NetError> {
    let version_dir_name = format!("{game_version}-forge-{loader_version}");
    let profile_filename = format!("forge-{game_version}-{loader_version}.json");
    super::save_installer_profile(instance_dir, meta_dir, &version_dir_name, &profile_filename)
}
