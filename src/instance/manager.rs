use std::path::PathBuf;

use chrono::Utc;
use thiserror::Error;

use crate::instance::models::{InstanceConfig, ModLoader};

#[derive(Debug, Error)]
pub enum InstanceError {
    #[error("Instance '{0}' already exists")]
    AlreadyExists(String),
    #[error("Instance '{0}' not found")]
    NotFound(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Download error: {0}")]
    Download(#[from] crate::net::NetError),
    #[error("Invalid instance name: {0}")]
    InvalidName(String),
}

pub struct InstanceManager {
    pub instances_dir: PathBuf,
    pub meta_dir: PathBuf,
    client: crate::net::HttpClient,
}

impl InstanceManager {
    pub fn new(instances_dir: impl Into<PathBuf>, meta_dir: impl Into<PathBuf>) -> Self {
        InstanceManager {
            instances_dir: instances_dir.into(),
            meta_dir: meta_dir.into(),
            client: crate::net::HttpClient::new(),
        }
    }

    pub async fn create(
        &self,
        name: &str,
        game_version: &str,
        loader: ModLoader,
        loader_version: Option<&str>,
    ) -> Result<InstanceConfig, InstanceError> {
        match validate_name(name) {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("Invalid instance name '{}': {}", name, e);
                return Err(e);
            }
        }

        let instance_dir = self.instances_dir.join(name);
        let instance_json = instance_dir.join("instance.json");

        if instance_json.exists() {
            tracing::error!("Instance '{}' already exists", name);
            return Err(InstanceError::AlreadyExists(name.to_string()));
        }

        if instance_dir.exists() && !instance_json.exists() {
            match std::fs::remove_dir_all(&instance_dir) {
                Ok(_) => {}
                Err(e) => {
                    tracing::error!(
                        "Failed to remove partial instance directory '{}': {}",
                        instance_dir.display(),
                        e
                    );
                    return Err(InstanceError::Io(e));
                }
            }
        }

        match std::fs::create_dir_all(&instance_dir) {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("Failed to create instance directory: {}", e);
                return Err(InstanceError::Io(e));
            }
        }

        let result = self
            .create_inner(name, game_version, loader, loader_version, &instance_dir)
            .await;

        if result.is_err() {
            match std::fs::remove_dir_all(&instance_dir) {
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(
                        "Failed to clean up partial instance directory '{}': {}",
                        instance_dir.display(),
                        e
                    );
                }
            }
        }

        result
    }

    async fn create_inner(
        &self,
        name: &str,
        game_version: &str,
        loader: ModLoader,
        loader_version: Option<&str>,
        instance_dir: &std::path::Path,
    ) -> Result<InstanceConfig, InstanceError> {
        let minecraft_dir = instance_dir.join(".minecraft");
        for subdir in &["mods", "config", "resourcepacks", "shaderpacks", "saves"] {
            match std::fs::create_dir_all(minecraft_dir.join(subdir)) {
                Ok(_) => {}
                Err(e) => {
                    tracing::error!("Failed to create directory: {}", e);
                    return Err(InstanceError::Io(e));
                }
            }
        }

        let launcher_profiles_path = minecraft_dir.join("launcher_profiles.json");
        if !launcher_profiles_path.exists() {
            match std::fs::write(&launcher_profiles_path, "{}") {
                Ok(_) => {}
                Err(e) => {
                    tracing::error!("Failed to create launcher_profiles.json: {}", e);
                    return Err(InstanceError::Io(e));
                }
            }
        }

        for meta_subdir in &[
            self.meta_dir.join("versions"),
            self.meta_dir.join("libraries"),
            self.meta_dir.join("assets").join("objects"),
            self.meta_dir.join("assets").join("indexes"),
        ] {
            match std::fs::create_dir_all(meta_subdir) {
                Ok(_) => {}
                Err(e) => {
                    tracing::error!("Failed to create meta directory: {}", e);
                    return Err(InstanceError::Io(e));
                }
            }
        }

        let manifest = match crate::net::mojang::fetch_version_manifest(&self.client).await {
            Ok(m) => m,
            Err(e) => {
                return Err(InstanceError::Download(e));
            }
        };

        let version_entry = match manifest.versions.iter().find(|v| v.id == game_version) {
            Some(v) => v,
            None => {
                let msg = format!("Minecraft version '{}' not found in manifest", game_version);
                tracing::error!("{}", msg);
                return Err(InstanceError::InvalidName(msg));
            }
        };

        let version_meta =
            match crate::net::mojang::fetch_version_meta(&self.client, version_entry).await {
                Ok(m) => m,
                Err(e) => {
                    return Err(InstanceError::Download(e));
                }
            };

        match crate::net::mojang::download_client_jar(&self.client, &version_meta, &self.meta_dir)
            .await
        {
            Ok(_) => {}
            Err(e) => {
                return Err(InstanceError::Download(e));
            }
        }

        let meta_json_path = self
            .meta_dir
            .join("versions")
            .join(game_version)
            .join("meta.json");
        match serde_json::to_string_pretty(&version_meta) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&meta_json_path, &json) {
                    tracing::warn!("Failed to save version meta: {}", e);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to serialize version meta: {}", e);
            }
        }

        match crate::net::mojang::download_libraries(&self.client, &version_meta, &self.meta_dir)
            .await
        {
            Ok(_) => {}
            Err(e) => {
                return Err(InstanceError::Download(e));
            }
        }

        match crate::net::mojang::download_assets(&self.client, &version_meta, &self.meta_dir).await
        {
            Ok(_) => {}
            Err(e) => {
                return Err(InstanceError::Download(e));
            }
        }

        let installer = crate::instance::loader::get_installer(loader);
        let effective_loader_version = match loader_version {
            Some(v) => v,
            None if loader == ModLoader::Vanilla => "vanilla",
            None => {
                tracing::error!("No loader version provided for {} loader", loader);
                return Err(InstanceError::InvalidName(format!(
                    "A loader version is required for {}",
                    loader
                )));
            }
        };
        match installer
            .install(
                &self.client,
                game_version,
                effective_loader_version,
                instance_dir,
                &self.meta_dir,
            )
            .await
        {
            Ok(_) => {}
            Err(e) => {
                return Err(InstanceError::Download(e));
            }
        }

        let config = InstanceConfig {
            name: name.to_string(),
            game_version: game_version.to_string(),
            loader,
            loader_version: loader_version.map(String::from),
            created: Utc::now(),
            last_played: None,
            java_path: None,
            memory_max: None,
            memory_min: None,
            jvm_args: vec![],
            resolution: None,
        };

        match self.save(&config) {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("Failed to save instance config: {}", e);
                return Err(e);
            }
        }

        crate::tui::progress::clear();
        Ok(config)
    }

    pub fn delete(&self, name: &str) -> Result<(), InstanceError> {
        let instance_dir = self.instances_dir.join(name);
        if !instance_dir.exists() {
            return Err(InstanceError::NotFound(name.to_string()));
        }
        match std::fs::remove_dir_all(&instance_dir) {
            Ok(_) => {
                if let Err(e) = crate::instance::desktop::remove(name) {
                    tracing::warn!("Failed to remove desktop shortcut for '{}': {}", name, e);
                }
                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to delete instance '{}': {}", name, e);
                Err(InstanceError::Io(e))
            }
        }
    }

    pub fn rename(&self, old_name: &str, new_name: &str) -> Result<(), InstanceError> {
        let new_name = new_name.trim();
        if new_name.is_empty() {
            return Err(InstanceError::InvalidName(
                "Name cannot be empty".to_string(),
            ));
        }
        if old_name == new_name {
            return Ok(());
        }
        let old_dir = self.instances_dir.join(old_name);
        let new_dir = self.instances_dir.join(new_name);
        if !old_dir.exists() {
            return Err(InstanceError::NotFound(old_name.to_string()));
        }
        if new_dir.exists() {
            return Err(InstanceError::AlreadyExists(new_name.to_string()));
        }
        std::fs::rename(&old_dir, &new_dir)?;

        let config_path = new_dir.join("instance.json");
        if let Ok(data) = std::fs::read_to_string(&config_path) {
            if let Ok(mut config) = serde_json::from_str::<InstanceConfig>(&data) {
                config.name = new_name.to_string();
                if let Ok(json) = serde_json::to_string_pretty(&config) {
                    let _ = std::fs::write(&config_path, json);
                }
                if let Err(e) = crate::instance::desktop::rename(old_name, &config) {
                    tracing::warn!("Failed to rename desktop shortcut: {}", e);
                }
            }
        }

        Ok(())
    }

    pub fn load_all(&self) -> Vec<InstanceConfig> {
        let mut instances = vec![];
        let read_dir = match std::fs::read_dir(&self.instances_dir) {
            Ok(rd) => rd,
            Err(e) => {
                tracing::error!("Failed to read instances directory: {}", e);
                return instances;
            }
        };
        for entry in read_dir {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::error!("Failed to read directory entry: {}", e);
                    continue;
                }
            };
            let config_path = entry.path().join("instance.json");
            if !config_path.exists() {
                continue;
            }
            let contents = match std::fs::read_to_string(&config_path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to read {}: {}", config_path.display(), e);
                    continue;
                }
            };
            match serde_json::from_str::<InstanceConfig>(&contents) {
                Ok(config) => instances.push(config),
                Err(e) => {
                    tracing::error!("Failed to parse {}: {}", config_path.display(), e);
                }
            }
        }
        instances
    }

    pub fn load_one(&self, name: &str) -> Result<InstanceConfig, InstanceError> {
        match validate_name(name) {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("Invalid instance name '{}': {}", name, e);
                return Err(e);
            }
        }

        let config_path = self.instances_dir.join(name).join("instance.json");
        if !config_path.exists() {
            tracing::error!("Instance '{}' not found", name);
            return Err(InstanceError::NotFound(name.to_string()));
        }

        let contents = match std::fs::read_to_string(&config_path) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Failed to read {}: {}", config_path.display(), e);
                return Err(InstanceError::Io(e));
            }
        };

        match serde_json::from_str::<InstanceConfig>(&contents) {
            Ok(config) => Ok(config),
            Err(e) => {
                tracing::error!("Failed to parse {}: {}", config_path.display(), e);
                Err(InstanceError::Json(e))
            }
        }
    }

    pub fn save(&self, instance: &InstanceConfig) -> Result<(), InstanceError> {
        let instance_dir = self.instances_dir.join(&instance.name);
        let config_path = instance_dir.join("instance.json");
        let json = match serde_json::to_string_pretty(instance) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!("Failed to serialize instance config: {}", e);
                return Err(InstanceError::Json(e));
            }
        };
        match std::fs::write(&config_path, &json) {
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::error!(
                    "Failed to write instance config to {}: {}",
                    config_path.display(),
                    e
                );
                Err(InstanceError::Io(e))
            }
        }
    }

    pub fn touch_last_played(&self, name: &str) -> Result<(), InstanceError> {
        let mut config = self.load_one(name)?;
        config.last_played = Some(chrono::Utc::now());
        self.save(&config)
    }
}

fn validate_name(name: &str) -> Result<(), InstanceError> {
    if name.is_empty() || name.len() > 64 {
        return Err(InstanceError::InvalidName(format!(
            "Name must be 1-64 chars, got: {:?}",
            name
        )));
    }
    if name.contains('/') || name.contains('\\') || name.contains('.') {
        return Err(InstanceError::InvalidName(format!(
            "Name contains invalid characters: {:?}",
            name
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::models::ModLoader;
    use std::path::PathBuf;

    fn test_manager() -> (InstanceManager, PathBuf) {
        let tmp = std::env::temp_dir().join(format!("mcl_test_{}", uuid_like()));
        let meta = std::env::temp_dir().join(format!("mcl_meta_test_{}", uuid_like()));
        std::fs::create_dir_all(&tmp).ok();
        std::fs::create_dir_all(&meta).ok();
        (InstanceManager::new(tmp.clone(), meta), tmp)
    }

    fn uuid_like() -> String {
        format!(
            "{:x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        )
    }

    #[test]
    fn test_validate_name_valid() {
        assert!(validate_name("my-instance").is_ok());
        assert!(validate_name("test_world").is_ok());
    }

    #[test]
    fn test_validate_name_invalid() {
        assert!(validate_name("").is_err());
        assert!(validate_name("path/traversal").is_err());
        assert!(validate_name(".hidden").is_err());
    }

    #[test]
    fn test_delete_nonexistent() {
        let (manager, tmp) = test_manager();
        let result = manager.delete("ghost-instance");
        assert!(matches!(result, Err(InstanceError::NotFound(_))));
        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_save_and_load_all() {
        let (manager, tmp) = test_manager();
        let instance_dir = tmp.join("test-save");
        std::fs::create_dir_all(&instance_dir).ok();

        let config = InstanceConfig {
            name: "test-save".to_string(),
            game_version: "1.20.1".to_string(),
            loader: ModLoader::Vanilla,
            loader_version: None,
            created: chrono::Utc::now(),
            last_played: None,
            java_path: None,
            memory_max: None,
            memory_min: None,
            jvm_args: vec![],
            resolution: None,
        };

        match manager.save(&config) {
            Ok(_) => {}
            Err(e) => panic!("save failed: {}", e),
        }

        let all = manager.load_all();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "test-save");
        assert_eq!(all[0].game_version, "1.20.1");

        std::fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn test_load_one_not_found() {
        let (manager, tmp) = test_manager();
        let result = manager.load_one("ghost-instance");
        assert!(matches!(result, Err(InstanceError::NotFound(_))));
        std::fs::remove_dir_all(&tmp).ok();
    }
}
