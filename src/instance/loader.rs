use std::path::Path;

use async_trait::async_trait;

use crate::instance::models::ModLoader;
use crate::net::fabric;
use crate::net::forge;
use crate::net::mojang;
use crate::net::neoforge;
use crate::net::quilt;
use crate::net::{HttpClient, NetError};

#[derive(Debug, Clone)]
pub struct GameVersion {
    pub id: String,
    pub stable: bool,
}

#[async_trait]
pub trait ModLoaderInstaller: Send + Sync {
    /// Returns the ModLoader variant this installer handles.
    fn loader_type(&self) -> ModLoader;

    async fn get_game_versions(&self, client: &HttpClient) -> Result<Vec<GameVersion>, NetError>;

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
        meta_dir: &Path,
    ) -> Result<(), NetError>;
}

pub struct VanillaInstaller;

#[async_trait]
impl ModLoaderInstaller for VanillaInstaller {
    fn loader_type(&self) -> ModLoader {
        ModLoader::Vanilla
    }

    async fn get_game_versions(&self, client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
        let manifest = match mojang::fetch_version_manifest(client).await {
            Ok(m) => m,
            Err(e) => {
                return Err(e);
            }
        };

        Ok(manifest
            .versions
            .into_iter()
            .map(|version| GameVersion {
                id: version.id,
                stable: version.version_type == "release",
            })
            .collect())
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
        _meta_dir: &Path,
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

    async fn get_game_versions(&self, client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
        match fabric::fetch_fabric_game_versions(client).await {
            Ok(versions) => Ok(versions),
            Err(e) => Err(e),
        }
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
        _instance_dir: &Path,
        meta_dir: &Path,
    ) -> Result<(), NetError> {
        let profile =
            match fabric::fetch_fabric_profile(client, game_version, loader_version).await {
                Ok(p) => p,
                Err(e) => {
                    return Err(e);
                }
            };

        match fabric::download_fabric_libraries(client, &profile, meta_dir).await {
            Ok(()) => {}
            Err(e) => {
                return Err(e);
            }
        }

        let profiles_dir = meta_dir.join("loader-profiles");
        match std::fs::create_dir_all(&profiles_dir) {
            Ok(_) => {
                let profile_path =
                    profiles_dir.join(format!("fabric-{}-{}.json", game_version, loader_version));
                match serde_json::to_string_pretty(&profile) {
                    Ok(json) => {
                        if let Err(e) = std::fs::write(&profile_path, &json) {
                            tracing::warn!("Failed to save Fabric profile: {}", e);
                        }
                    }
                    Err(e) => tracing::warn!("Failed to serialize Fabric profile: {}", e),
                }
            }
            Err(e) => tracing::warn!("Failed to create loader-profiles dir: {}", e),
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

    async fn get_game_versions(&self, client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
        match forge::fetch_forge_game_versions(client).await {
            Ok(versions) => Ok(versions),
            Err(e) => Err(e),
        }
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
        meta_dir: &Path,
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

        let java_path = crate::net::detect_java_path();
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

        let forge_ver_json = instance_dir
            .join(".minecraft")
            .join("versions")
            .join(format!("{}-forge-{}", game_version, loader_version))
            .join(format!("{}-forge-{}.json", game_version, loader_version));
        if forge_ver_json.exists() {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct ForgeVersionJson {
                main_class: String,
                #[serde(default)]
                libraries: Vec<ForgeVersionLib>,
            }
            #[derive(serde::Deserialize)]
            struct ForgeVersionLib {
                name: String,
            }
            match std::fs::read(&forge_ver_json)
                .ok()
                .and_then(|b| serde_json::from_slice::<ForgeVersionJson>(&b).ok())
            {
                Some(forge_ver) => {
                    let profile_dir = meta_dir.join("loader-profiles");
                    match std::fs::create_dir_all(&profile_dir) {
                        Ok(_) => {
                            let profile_path = profile_dir
                                .join(format!("forge-{}-{}.json", game_version, loader_version));
                            let libs: Vec<serde_json::Value> = forge_ver
                                .libraries
                                .iter()
                                .map(|l| serde_json::json!({"name": l.name}))
                                .collect();
                            let json_val = serde_json::json!({
                                "mainClass": forge_ver.main_class,
                                "libraries": libs
                            });
                            match serde_json::to_string_pretty(&json_val) {
                                Ok(json) => {
                                    if let Err(e) = std::fs::write(&profile_path, &json) {
                                        tracing::warn!("Failed to save Forge profile: {}", e);
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to serialize Forge profile: {}", e)
                                }
                            }
                        }
                        Err(e) => tracing::warn!("Failed to create loader-profiles dir: {}", e),
                    }
                }
                None => tracing::warn!(
                    "Could not read Forge version JSON at {}",
                    forge_ver_json.display()
                ),
            }
        } else {
            tracing::warn!(
                "Forge version JSON not found at {} — profile not saved",
                forge_ver_json.display()
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

    async fn get_game_versions(&self, client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
        match quilt::fetch_quilt_game_versions(client).await {
            Ok(versions) => Ok(versions),
            Err(e) => Err(e),
        }
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
        _instance_dir: &Path,
        meta_dir: &Path,
    ) -> Result<(), NetError> {
        let profile =
            match quilt::fetch_quilt_profile(client, game_version, loader_version).await {
                Ok(p) => p,
                Err(e) => {
                    return Err(e);
                }
            };

        match quilt::download_quilt_libraries(client, &profile, meta_dir).await {
            Ok(()) => {}
            Err(e) => {
                return Err(e);
            }
        }

        let profiles_dir = meta_dir.join("loader-profiles");
        match std::fs::create_dir_all(&profiles_dir) {
            Ok(_) => {
                let profile_path =
                    profiles_dir.join(format!("quilt-{}-{}.json", game_version, loader_version));
                match serde_json::to_string_pretty(&profile) {
                    Ok(json) => {
                        if let Err(e) = std::fs::write(&profile_path, &json) {
                            tracing::warn!("Failed to save Quilt profile: {}", e);
                        }
                    }
                    Err(e) => tracing::warn!("Failed to serialize Quilt profile: {}", e),
                }
            }
            Err(e) => tracing::warn!("Failed to create loader-profiles dir: {}", e),
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

    async fn get_game_versions(&self, client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
        match neoforge::fetch_neoforge_game_versions(client).await {
            Ok(versions) => Ok(versions),
            Err(e) => Err(e),
        }
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
        meta_dir: &Path,
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

        let java_path = crate::net::detect_java_path();
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

        let neo_ver_json = instance_dir
            .join(".minecraft")
            .join("versions")
            .join(format!("neoforge-{}", loader_version))
            .join(format!("neoforge-{}.json", loader_version));
        if neo_ver_json.exists() {
            #[derive(serde::Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct ForgeVersionJson {
                main_class: String,
                #[serde(default)]
                libraries: Vec<ForgeVersionLib>,
            }
            #[derive(serde::Deserialize)]
            struct ForgeVersionLib {
                name: String,
            }
            match std::fs::read(&neo_ver_json)
                .ok()
                .and_then(|b| serde_json::from_slice::<ForgeVersionJson>(&b).ok())
            {
                Some(neo_ver) => {
                    let profile_dir = meta_dir.join("loader-profiles");
                    match std::fs::create_dir_all(&profile_dir) {
                        Ok(_) => {
                            let profile_path =
                                profile_dir.join(format!("neoforge-{}.json", loader_version));
                            let libs: Vec<serde_json::Value> = neo_ver
                                .libraries
                                .iter()
                                .map(|l| serde_json::json!({"name": l.name}))
                                .collect();
                            let json_val = serde_json::json!({
                                "mainClass": neo_ver.main_class,
                                "libraries": libs
                            });
                            match serde_json::to_string_pretty(&json_val) {
                                Ok(json) => {
                                    if let Err(e) = std::fs::write(&profile_path, &json) {
                                        tracing::warn!("Failed to save NeoForge profile: {}", e);
                                    }
                                }
                                Err(e) => tracing::warn!(
                                    "Failed to serialize NeoForge profile: {}",
                                    e
                                ),
                            }
                        }
                        Err(e) => tracing::warn!("Failed to create loader-profiles dir: {}", e),
                    }
                }
                None => tracing::warn!(
                    "Could not read NeoForge version JSON at {}",
                    neo_ver_json.display()
                ),
            }
        } else {
            tracing::warn!(
                "NeoForge version JSON not found at {} — profile not saved",
                neo_ver_json.display()
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
        let meta = std::env::temp_dir().join("mcl_test_meta");
        match installer.install(&client, "1.20.1", "vanilla", &tmp, &meta).await {
            Ok(()) => {}
            Err(e) => assert!(false, "install failed: {}", e),
        }
    }

    #[tokio::test]
    async fn test_vanilla_get_game_versions() {
        let client = HttpClient::new();
        let installer = VanillaInstaller;
        match installer.get_game_versions(&client).await {
            Ok(versions) => {
                assert!(!versions.is_empty());
                assert!(versions.iter().any(|version| version.id == "1.20.1"));
            }
            Err(e) => assert!(false, "get_game_versions failed: {}", e),
        }
    }
}
