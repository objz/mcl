// forge installation. modern forge runs a java installer, old forge (pre-1.13)
// can't run headless so we extract the profile and libraries from the jar
// directly. the installer jar gets cleaned up either way.

use std::path::Path;

use async_trait::async_trait;

use super::{GameVersion, InstallError, ModLoaderInstaller};
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
        tracing::debug!("Fetching Forge versions for Minecraft {}", game_version);
        let versions = forge_api::fetch_forge_versions(client, game_version).await?;
        tracing::debug!(
            "Fetched {} Forge version(s) for Minecraft {}",
            versions.len(),
            game_version
        );
        Ok(versions)
    }

    async fn install(
        &self,
        client: &HttpClient,
        game_version: &str,
        loader_version: &str,
        instance_dir: &Path,
        meta_dir: &Path,
    ) -> Result<(), InstallError> {
        let installer_jar = instance_dir.join(".minecraft").join("forge-installer.jar");
        tracing::info!(
            "Installing Forge {} for Minecraft {}",
            loader_version,
            game_version
        );
        tracing::debug!("Forge installer path: {}", installer_jar.display());

        forge_api::download_forge_installer(client, game_version, loader_version, &installer_jar)
            .await?;

        let profile_filename = format!("forge-{game_version}-{loader_version}.json");

        if forge_api::has_legacy_install_profile(&installer_jar) {
            // old forge: no --installClient support, extract directly from jar
            tracing::debug!("Forge installer uses legacy install_profile.json path");
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
            tracing::debug!("Running Forge installer with Java {}", java_path);
            if let Err(e) =
                forge_api::run_forge_installer(&installer_jar, instance_dir, &java_path).await
            {
                let _ = tokio::fs::remove_file(&installer_jar).await;
                return Err(InstallError::Installer(e));
            }

            // extract the profile from what the installer just wrote to disk
            save_forge_profile(instance_dir, meta_dir, game_version, loader_version)
                .map_err(InstallError::Installer)?;
        }

        if let Err(e) = tokio::fs::remove_file(&installer_jar).await {
            tracing::warn!("Failed to remove Forge installer JAR: {}", e);
        }

        tracing::debug!(
            "Installed Forge {} for Minecraft {}",
            loader_version,
            game_version
        );
        Ok(())
    }
}

fn save_forge_profile(
    instance_dir: &Path,
    meta_dir: &Path,
    game_version: &str,
    loader_version: &str,
) -> Result<(), super::InstallerError> {
    let version_dir_name = format!("{game_version}-forge-{loader_version}");
    let profile_filename = format!("forge-{game_version}-{loader_version}.json");
    super::save_installer_profile(instance_dir, meta_dir, &version_dir_name, &profile_filename)
}
