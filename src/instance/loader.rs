use std::path::Path;

use async_trait::async_trait;

use crate::instance::models::ModLoader;
use crate::net::fabric;
use crate::net::forge;
use crate::net::neoforge;
use crate::net::quilt;
use crate::net::{HttpClient, NetError};

#[async_trait]
pub trait ModLoaderInstaller: Send + Sync {
    /// Returns the ModLoader variant this installer handles.
    fn loader_type(&self) -> ModLoader;

    /// Returns available loader versions for the given Minecraft version.
    /// For Vanilla, returns a single placeholder entry.
    async fn get_versions(
        &self,
        client: &HttpClient,
        game_version: &str,
    ) -> Result<Vec<String>, NetError>;

    /// Downloads and installs the loader into the given instance directory.
    /// For Vanilla, this is a no-op since Minecraft files are downloaded separately.
    async fn install(
        &self,
        client: &HttpClient,
        game_version: &str,
        loader_version: &str,
        instance_dir: &Path,
    ) -> Result<(), NetError>;
}

pub struct VanillaInstaller;

#[async_trait]
impl ModLoaderInstaller for VanillaInstaller {
    fn loader_type(&self) -> ModLoader {
        ModLoader::Vanilla
    }

    async fn get_versions(
        &self,
        _client: &HttpClient,
        _game_version: &str,
    ) -> Result<Vec<String>, NetError> {
        Ok(vec!["vanilla".to_string()])
    }

    async fn install(
        &self,
        _client: &HttpClient,
        _game_version: &str,
        _loader_version: &str,
        _instance_dir: &Path,
    ) -> Result<(), NetError> {
        // Vanilla: no loader to install; Minecraft files downloaded by Mojang API separately
        Ok(())
    }
}

pub struct FabricInstaller;

#[async_trait]
impl ModLoaderInstaller for FabricInstaller {
    fn loader_type(&self) -> ModLoader {
        ModLoader::Fabric
    }

    async fn get_versions(
        &self,
        client: &HttpClient,
        game_version: &str,
    ) -> Result<Vec<String>, NetError> {
        let loader_versions = match fabric::fetch_fabric_versions(client, game_version).await {
            Ok(v) => v,
            Err(e) => {
                return Err(e);
            }
        };
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
        instance_dir: &Path,
    ) -> Result<(), NetError> {
        let profile =
            match fabric::fetch_fabric_profile(client, game_version, loader_version).await {
                Ok(p) => p,
                Err(e) => {
                    return Err(e);
                }
            };

        match fabric::download_fabric_libraries(client, &profile, instance_dir).await {
            Ok(()) => {}
            Err(e) => {
                return Err(e);
            }
        }

        Ok(())
    }
}

pub struct ForgeInstaller;

#[async_trait]
impl ModLoaderInstaller for ForgeInstaller {
    fn loader_type(&self) -> ModLoader {
        ModLoader::Forge
    }

    async fn get_versions(
        &self,
        client: &HttpClient,
        game_version: &str,
    ) -> Result<Vec<String>, NetError> {
        match forge::fetch_forge_versions(client, game_version).await {
            Ok(v) => Ok(v),
            Err(e) => {
                Err(e)
            }
        }
    }

    async fn install(
        &self,
        client: &HttpClient,
        game_version: &str,
        loader_version: &str,
        instance_dir: &Path,
    ) -> Result<(), NetError> {
        let installer_jar = instance_dir.join(".minecraft").join("forge-installer.jar");

        match forge::download_forge_installer(client, game_version, loader_version, &installer_jar)
            .await
        {
            Ok(()) => {}
            Err(e) => {
                return Err(e);
            }
        }

        let java_path = forge::detect_java_path();
        match forge::run_forge_installer(&installer_jar, instance_dir, &java_path).await {
            Ok(()) => {}
            Err(e) => {
                let _ = tokio::fs::remove_file(&installer_jar).await;
                return Err(e);
            }
        }

        if let Err(e) = tokio::fs::remove_file(&installer_jar).await {
            tracing::error!(
                "Failed to remove Forge installer JAR {}: {}",
                installer_jar.display(),
                e
            );
        }

        Ok(())
    }
}

pub struct QuiltInstaller;

#[async_trait]
impl ModLoaderInstaller for QuiltInstaller {
    fn loader_type(&self) -> ModLoader {
        ModLoader::Quilt
    }

    async fn get_versions(
        &self,
        client: &HttpClient,
        game_version: &str,
    ) -> Result<Vec<String>, NetError> {
        let loader_versions = match quilt::fetch_quilt_versions(client, game_version).await {
            Ok(v) => v,
            Err(e) => {
                return Err(e);
            }
        };
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
        instance_dir: &Path,
    ) -> Result<(), NetError> {
        let profile =
            match quilt::fetch_quilt_profile(client, game_version, loader_version).await {
                Ok(p) => p,
                Err(e) => {
                    return Err(e);
                }
            };

        match quilt::download_quilt_libraries(client, &profile, instance_dir).await {
            Ok(()) => {}
            Err(e) => {
                return Err(e);
            }
        }

        Ok(())
    }
}

pub struct NeoForgeInstaller;

#[async_trait]
impl ModLoaderInstaller for NeoForgeInstaller {
    fn loader_type(&self) -> ModLoader {
        ModLoader::NeoForge
    }

    async fn get_versions(
        &self,
        client: &HttpClient,
        game_version: &str,
    ) -> Result<Vec<String>, NetError> {
        match neoforge::fetch_neoforge_versions(client, game_version).await {
            Ok(v) => Ok(v),
            Err(e) => {
                Err(e)
            }
        }
    }

    async fn install(
        &self,
        client: &HttpClient,
        _game_version: &str,
        loader_version: &str,
        instance_dir: &Path,
    ) -> Result<(), NetError> {
        let installer_jar = instance_dir
            .join(".minecraft")
            .join("neoforge-installer.jar");

        match neoforge::download_neoforge_installer(client, loader_version, &installer_jar).await {
            Ok(()) => {}
            Err(e) => {
                return Err(e);
            }
        }

        let java_path = neoforge::detect_java_path();
        match neoforge::run_neoforge_installer(&installer_jar, instance_dir, &java_path).await {
            Ok(()) => {}
            Err(e) => {
                let _ = tokio::fs::remove_file(&installer_jar).await;
                return Err(e);
            }
        }

        if let Err(e) = tokio::fs::remove_file(&installer_jar).await {
            tracing::error!(
                "Failed to remove NeoForge installer JAR {}: {}",
                installer_jar.display(),
                e
            );
        }

        Ok(())
    }
}

pub fn get_installer(loader: ModLoader) -> Box<dyn ModLoaderInstaller + Send + Sync> {
    match loader {
        ModLoader::Vanilla => Box::new(VanillaInstaller),
        ModLoader::Fabric => Box::new(FabricInstaller),
        ModLoader::Forge => Box::new(ForgeInstaller),
        ModLoader::NeoForge => Box::new(NeoForgeInstaller),
        ModLoader::Quilt => Box::new(QuiltInstaller),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vanilla_factory() {
        let installer = get_installer(ModLoader::Vanilla);
        assert_eq!(installer.loader_type(), ModLoader::Vanilla);
    }

    #[tokio::test]
    async fn test_vanilla_get_versions() {
        let client = HttpClient::new();
        let installer = VanillaInstaller;
        match installer.get_versions(&client, "1.20.1").await {
            Ok(versions) => {
                assert!(!versions.is_empty());
                assert_eq!(versions[0], "vanilla");
            }
            Err(e) => assert!(false, "get_versions failed: {}", e),
        }
    }

    #[tokio::test]
    async fn test_vanilla_install_noop() {
        let client = HttpClient::new();
        let installer = VanillaInstaller;
        let tmp = std::env::temp_dir().join("mcl_test_vanilla_install");
        match installer.install(&client, "1.20.1", "vanilla", &tmp).await {
            Ok(()) => {}
            Err(e) => assert!(false, "install failed: {}", e),
        }
    }
}
