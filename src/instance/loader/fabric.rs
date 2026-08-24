// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// fabric installation. the nice one: just fetch a profile json and download
// the libraries. no java installer nonsense required.

use std::path::Path;

use async_trait::async_trait;

use super::{GameVersion, InstallError, InstallerError, ModLoaderInstaller};
use crate::instance::models::ModLoader;
use crate::net::{HttpClient, NetError, fabric as fabric_api};

pub struct FabricInstaller;

#[async_trait]
impl ModLoaderInstaller for FabricInstaller {
    fn loader_type(&self) -> ModLoader {
        ModLoader::Fabric
    }

    async fn get_game_versions(&self, client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
        fabric_api::fetch_fabric_game_versions(client).await
    }

    async fn get_versions(
        &self,
        client: &HttpClient,
        game_version: &str,
    ) -> Result<Vec<String>, NetError> {
        tracing::debug!(
            "Fetching Fabric loader versions for Minecraft {}",
            game_version
        );
        let loader_versions = fabric_api::fetch_fabric_versions(client, game_version).await?;
        tracing::debug!(
            "Fetched {} Fabric loader version(s) for Minecraft {}",
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
            "Installing Fabric {} for Minecraft {}",
            loader_version,
            game_version
        );
        let (profile, raw_bytes) =
            fabric_api::fetch_fabric_profile_with_raw(client, game_version, loader_version).await?;
        tracing::debug!(
            "Fetched Fabric profile {} with {} libraries",
            profile.id,
            profile.libraries.len()
        );
        fabric_api::download_fabric_libraries(client, &profile, meta_dir).await?;
        super::save_profile_bytes(
            meta_dir,
            &format!("fabric-{game_version}-{loader_version}.json"),
            &raw_bytes,
        )
        .map_err(InstallerError::from)?;
        tracing::debug!(
            "Saved Fabric profile for Minecraft {} loader {}",
            game_version,
            loader_version
        );
        Ok(())
    }
}
