// CRUD for instances: create, delete, rename, load, save.
// creation is the heavy one since it downloads the game, assets, and libraries.

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
    /// shared across all instances: versions, libraries, assets
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
        validate_name(name)?;

        let instance_dir = self.instances_dir.join(name);
        let instance_json = instance_dir.join("instance.json");

        if instance_json.exists() {
            return Err(InstanceError::AlreadyExists(name.to_string()));
        }

        // leftover directory without config = botched previous creation, nuke it
        if instance_dir.exists() && !instance_json.exists() {
            std::fs::remove_dir_all(&instance_dir)?;
        }

        std::fs::create_dir_all(&instance_dir)?;

        let result = self
            .create_inner(name, game_version, loader, loader_version, &instance_dir)
            .await;

        // clean up on failure so there's no half-baked instance left around
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&instance_dir);
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
            std::fs::create_dir_all(minecraft_dir.join(subdir))?;
        }

        // forge insists on this file existing, even if it's empty json. thanks forge.
        let launcher_profiles_path = minecraft_dir.join("launcher_profiles.json");
        if !launcher_profiles_path.exists() {
            std::fs::write(&launcher_profiles_path, "{}")?;
        }

        for meta_subdir in &[
            self.meta_dir.join("versions"),
            self.meta_dir.join("libraries"),
            self.meta_dir.join("assets").join("objects"),
            self.meta_dir.join("assets").join("indexes"),
        ] {
            std::fs::create_dir_all(meta_subdir)?;
        }

        let manifest = crate::net::mojang::fetch_version_manifest(&self.client).await?;

        let version_entry = match manifest.versions.iter().find(|v| v.id == game_version) {
            Some(v) => v,
            None => {
                return Err(InstanceError::InvalidName(format!(
                    "Minecraft version '{}' not found in manifest",
                    game_version
                )));
            }
        };

        let (version_meta, raw_meta_bytes) =
            crate::net::mojang::fetch_version_meta_with_raw(&self.client, version_entry).await?;

        crate::net::mojang::download_client_jar(&self.client, &version_meta, &self.meta_dir)
            .await?;

        let meta_json_path = self
            .meta_dir
            .join("versions")
            .join(game_version)
            .join("meta.json");
        if let Some(parent) = meta_json_path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!("Failed to ensure meta dir exists: {}", e);
        }
        if let Err(e) = std::fs::write(&meta_json_path, &raw_meta_bytes) {
            tracing::warn!("Failed to save version meta: {}", e);
        }

        crate::net::mojang::download_libraries(&self.client, &version_meta, &self.meta_dir).await?;

        crate::net::mojang::download_assets(&self.client, &version_meta, &self.meta_dir).await?;

        let installer = crate::instance::loader::get_installer(loader);
        let effective_loader_version = match loader_version {
            Some(v) => v,
            None if loader == ModLoader::Vanilla => "vanilla",
            None => {
                return Err(InstanceError::InvalidName(format!(
                    "A loader version is required for {}",
                    loader
                )));
            }
        };
        installer
            .install(
                &self.client,
                game_version,
                effective_loader_version,
                instance_dir,
                &self.meta_dir,
            )
            .await?;

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

        self.save(&config)?;

        crate::tui::progress::clear();
        Ok(config)
    }

    pub fn delete(&self, name: &str) -> Result<(), InstanceError> {
        let instance_dir = self.instances_dir.join(name);
        if !instance_dir.exists() {
            return Err(InstanceError::NotFound(name.to_string()));
        }
        std::fs::remove_dir_all(&instance_dir)?;
        if let Err(e) = crate::instance::desktop::remove(name) {
            tracing::warn!("Failed to remove desktop shortcut for '{}': {}", name, e);
        }
        Ok(())
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
        if let Ok(data) = std::fs::read_to_string(&config_path)
            && let Ok(mut config) = serde_json::from_str::<InstanceConfig>(&data)
        {
            config.name = new_name.to_string();
            if let Ok(json) = serde_json::to_string_pretty(&config) {
                let _ = std::fs::write(&config_path, json);
            }
            if let Err(e) = crate::instance::desktop::rename(old_name, &config) {
                tracing::warn!("Failed to rename desktop shortcut: {}", e);
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
        validate_name(name)?;

        let config_path = self.instances_dir.join(name).join("instance.json");
        if !config_path.exists() {
            return Err(InstanceError::NotFound(name.to_string()));
        }

        let contents = std::fs::read_to_string(&config_path)?;
        Ok(serde_json::from_str::<InstanceConfig>(&contents)?)
    }

    pub fn save(&self, instance: &InstanceConfig) -> Result<(), InstanceError> {
        let instance_dir = self.instances_dir.join(&instance.name);
        let config_path = instance_dir.join("instance.json");
        let json = serde_json::to_string_pretty(instance)?;
        std::fs::write(&config_path, &json)?;
        Ok(())
    }

    pub fn touch_last_played(&self, name: &str) -> Result<(), InstanceError> {
        let mut config = self.load_one(name)?;
        config.last_played = Some(chrono::Utc::now());
        self.save(&config)
    }
}

// guard against path traversal and other filesystem shenanigans
fn validate_name(name: &str) -> Result<(), InstanceError> {
    if name.is_empty() || name.len() > 64 {
        return Err(InstanceError::InvalidName(format!(
            "Name must be 1-64 chars, got: {:?}",
            name
        )));
    }
    if name.contains('/') || name.contains('\\') || name.starts_with('.') {
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
    use tempfile::TempDir;

    // tmp owns the temp directory; its Drop impl cleans up everything when
    // the test ends. the returned InstanceManager points at tmp.path() so
    // tests can join("name") off of either to refer to the same locations.
    fn test_manager() -> (InstanceManager, TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let meta = tmp.path().join("meta");
        std::fs::create_dir_all(&meta).unwrap();
        (InstanceManager::new(tmp.path().to_path_buf(), meta), tmp)
    }

    fn dummy_config(name: &str) -> InstanceConfig {
        InstanceConfig {
            name: name.to_string(),
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
        }
    }

    #[test]
    fn validate_name_accepts_safe_names() {
        assert!(validate_name("my-instance").is_ok());
        assert!(validate_name("test_world").is_ok());
    }

    #[test]
    fn validate_name_rejects_empty_traversal_and_hidden() {
        assert!(validate_name("").is_err());
        assert!(validate_name("path/traversal").is_err());
        assert!(validate_name(".hidden").is_err());
    }

    #[test]
    fn delete_missing_instance_returns_not_found() {
        let (manager, _tmp) = test_manager();
        let result = manager.delete("ghost-instance");
        assert!(matches!(result, Err(InstanceError::NotFound(_))));
    }

    #[test]
    fn save_then_load_all_round_trips_config() {
        let (manager, tmp) = test_manager();
        std::fs::create_dir_all(tmp.path().join("test-save")).unwrap();
        manager.save(&dummy_config("test-save")).expect("save");

        let all = manager.load_all();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "test-save");
        assert_eq!(all[0].game_version, "1.20.1");
    }

    #[test]
    fn load_all_accepts_numeric_memory() {
        let (manager, tmp) = test_manager();
        let instance_dir = tmp.path().join("test-memory");
        std::fs::create_dir_all(&instance_dir).unwrap();
        std::fs::write(
            instance_dir.join("instance.json"),
            r#"{
  "name": "test-memory",
  "game_version": "1.7.10",
  "loader": "forge",
  "loader_version": "10.13.4.1614",
  "created": "2026-04-20T18:04:25.567993893Z",
  "memory_max": 8,
  "memory_min": 512
}"#,
        )
        .expect("write config");

        let all = manager.load_all();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].memory_max.as_deref(), Some("8G"));
        assert_eq!(all[0].memory_min.as_deref(), Some("512M"));
    }

    #[test]
    fn load_one_missing_returns_not_found() {
        let (manager, _tmp) = test_manager();
        let result = manager.load_one("ghost-instance");
        assert!(matches!(result, Err(InstanceError::NotFound(_))));
    }

    #[test]
    fn rename_moves_dir_and_updates_config_name() {
        let (manager, tmp) = test_manager();
        let old_dir = tmp.path().join("old-name");
        std::fs::create_dir_all(&old_dir).unwrap();
        manager.save(&dummy_config("old-name")).expect("save");

        manager.rename("old-name", "new-name").expect("rename");

        assert!(!old_dir.exists(), "old dir should be gone");
        let new_dir = tmp.path().join("new-name");
        assert!(new_dir.exists(), "new dir should exist");
        let reloaded = manager.load_one("new-name").expect("load_one new-name");
        assert_eq!(reloaded.name, "new-name");
    }

    #[test]
    fn rename_to_same_name_is_noop() {
        let (manager, tmp) = test_manager();
        let dir = tmp.path().join("same");
        std::fs::create_dir_all(&dir).unwrap();
        manager.save(&dummy_config("same")).expect("save");
        manager.rename("same", "same").expect("noop rename");
        assert!(dir.exists());
    }

    #[test]
    fn rename_empty_target_rejects() {
        let (manager, tmp) = test_manager();
        std::fs::create_dir_all(tmp.path().join("orig")).unwrap();
        manager.save(&dummy_config("orig")).expect("save");
        let err = manager.rename("orig", "   ").unwrap_err();
        assert!(matches!(err, InstanceError::InvalidName(_)));
    }

    #[test]
    fn rename_missing_source_errors() {
        let (manager, _tmp) = test_manager();
        let err = manager.rename("ghost", "anything").unwrap_err();
        assert!(matches!(err, InstanceError::NotFound(_)));
    }

    #[test]
    fn rename_target_exists_errors() {
        let (manager, tmp) = test_manager();
        std::fs::create_dir_all(tmp.path().join("source")).unwrap();
        std::fs::create_dir_all(tmp.path().join("collision")).unwrap();
        manager.save(&dummy_config("source")).expect("save src");
        manager.save(&dummy_config("collision")).expect("save dst");
        let err = manager.rename("source", "collision").unwrap_err();
        assert!(matches!(err, InstanceError::AlreadyExists(_)));
    }

    #[test]
    fn touch_last_played_updates_field() {
        let (manager, tmp) = test_manager();
        std::fs::create_dir_all(tmp.path().join("ticker")).unwrap();
        manager.save(&dummy_config("ticker")).expect("save");
        assert!(manager.load_one("ticker").unwrap().last_played.is_none());

        manager.touch_last_played("ticker").expect("touch");
        let reloaded = manager.load_one("ticker").unwrap();
        let stamp = reloaded
            .last_played
            .expect("last_played should be Some now");
        let age = chrono::Utc::now() - stamp;
        assert!(
            age.num_seconds().abs() < 5,
            "last_played should be roughly now, got age {age:?}"
        );
    }
}
