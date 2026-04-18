// mod loader installation. each loader (fabric, forge, neoforge, quilt, vanilla)
// implements the same trait so the UI can treat them uniformly: pick game version,
// pick loader version, install. the actual installation strategies differ wildly
// though (fabric/quilt just download jars, forge/neoforge run a whole java installer).

mod fabric;
mod forge;
mod neoforge;
mod quilt;
mod vanilla;

use std::path::Path;

use async_trait::async_trait;

use crate::instance::models::ModLoader;
use crate::net::{HttpClient, NetError};

pub use vanilla::VanillaInstaller;

#[derive(Debug, Clone)]
pub struct GameVersion {
    pub id: String,
    pub stable: bool,
}

#[async_trait]
pub trait ModLoaderInstaller: Send + Sync {
    fn loader_type(&self) -> ModLoader;

    async fn get_game_versions(&self, client: &HttpClient) -> Result<Vec<GameVersion>, NetError>;

    async fn get_versions(
        &self,
        client: &HttpClient,
        game_version: &str,
    ) -> Result<Vec<String>, NetError>;

    async fn install(
        &self,
        client: &HttpClient,
        game_version: &str,
        loader_version: &str,
        instance_dir: &Path,
        meta_dir: &Path,
    ) -> Result<(), NetError>;
}

pub(crate) fn save_profile_json(
    meta_dir: &Path,
    filename: &str,
    profile: &impl serde::Serialize,
) -> Result<(), NetError> {
    let profiles_dir = meta_dir.join("loader-profiles");
    std::fs::create_dir_all(&profiles_dir)?;
    let profile_path = profiles_dir.join(filename);
    let json = serde_json::to_string_pretty(profile)
        .map_err(|e| NetError::Parse(format!("Failed to serialize profile {filename}: {e}")))?;
    std::fs::write(&profile_path, &json)?;
    Ok(())
}

// used by forge/neoforge. their java installer drops a version json into
// .minecraft/versions/. parses that to extract the main class and library
// list, then saves a stripped-down profile for use at launch time.
pub(crate) fn save_installer_profile(
    instance_dir: &Path,
    meta_dir: &Path,
    version_dir_name: &str,
    profile_filename: &str,
) -> Result<(), NetError> {
    let ver_json_path = instance_dir
        .join(".minecraft")
        .join("versions")
        .join(version_dir_name)
        .join(format!("{version_dir_name}.json"));

    if !ver_json_path.exists() {
        return Err(NetError::Parse(format!(
            "Version JSON not found at {}",
            ver_json_path.display()
        )));
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct InstallerVersionJson {
        main_class: String,
        #[serde(default)]
        libraries: Vec<InstallerLib>,
    }
    #[derive(serde::Deserialize)]
    struct InstallerLib {
        name: String,
    }

    let raw = std::fs::read(&ver_json_path)?;
    let ver: InstallerVersionJson = serde_json::from_slice(&raw).map_err(|e| {
        NetError::Parse(format!(
            "Invalid version JSON at {}: {e}",
            ver_json_path.display()
        ))
    })?;

    let libs: Vec<serde_json::Value> = ver
        .libraries
        .iter()
        .map(|l| serde_json::json!({"name": l.name}))
        .collect();
    let json_val = serde_json::json!({
        "mainClass": ver.main_class,
        "libraries": libs
    });

    save_profile_json(meta_dir, profile_filename, &json_val)
}

pub fn get_installer(loader: ModLoader) -> Box<dyn ModLoaderInstaller + Send + Sync> {
    match loader {
        ModLoader::Vanilla => Box::new(vanilla::VanillaInstaller),
        ModLoader::Fabric => Box::new(fabric::FabricInstaller),
        ModLoader::Forge => Box::new(forge::ForgeInstaller),
        ModLoader::NeoForge => Box::new(neoforge::NeoForgeInstaller),
        ModLoader::Quilt => Box::new(quilt::QuiltInstaller),
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
        let versions = installer.get_versions(&client, "1.20.1").await.unwrap();
        assert!(!versions.is_empty());
        assert_eq!(versions[0], "vanilla");
    }

    #[tokio::test]
    async fn test_vanilla_install_noop() {
        let client = HttpClient::new();
        let installer = VanillaInstaller;
        let tmp = std::env::temp_dir().join("mcl_test_vanilla_install");
        let meta = std::env::temp_dir().join("mcl_test_meta");
        installer
            .install(&client, "1.20.1", "vanilla", &tmp, &meta)
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "hits live Mojang API"]
    async fn test_vanilla_get_game_versions() {
        let client = HttpClient::new();
        let installer = VanillaInstaller;
        let versions = installer.get_game_versions(&client).await.unwrap();
        assert!(!versions.is_empty());
        assert!(versions.iter().any(|v| v.id == "1.20.1"));
    }
}
