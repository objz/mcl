// forge installation. modern forge runs a java installer, old forge (pre-1.13)
// can't run headless so we extract the profile and libraries from the jar
// directly. the installer jar gets cleaned up either way.

use std::path::Path;

use async_trait::async_trait;

use super::{GameVersion, ModLoaderInstaller};
use crate::instance::models::ModLoader;
use crate::net::{HttpClient, NetError, forge as forge_api};

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

        let profile_filename = format!("forge-{game_version}-{loader_version}.json");

        if forge_api::has_legacy_install_profile(&installer_jar) {
            // old forge: no --installClient support, extract directly from jar
            if let Err(e) = forge_api::install_forge_from_profile(
                client,
                &installer_jar,
                meta_dir,
                &profile_filename,
            )
            .await
            {
                let _ = tokio::fs::remove_file(&installer_jar).await;
                return Err(e);
            }
        } else {
            // modern forge: run the java installer
            let java_path = crate::config::SETTINGS
                .paths
                .effective_java_path()
                .map(str::to_owned)
                .unwrap_or_else(crate::net::detect_java_path);
            if let Err(e) =
                forge_api::run_forge_installer(&installer_jar, instance_dir, &java_path).await
            {
                let _ = tokio::fs::remove_file(&installer_jar).await;
                return Err(e);
            }

            // extract the profile from what the installer just wrote to disk
            save_forge_profile(instance_dir, meta_dir, game_version, loader_version)?;
        }

        if let Err(e) = tokio::fs::remove_file(&installer_jar).await {
            tracing::warn!("Failed to remove Forge installer JAR: {}", e);
        }

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
