use config::{Config as ConfigLoader, ConfigError, File};
use dirs_next::config_dir;
use once_cell::sync::Lazy;
use std::fs;
use std::path::PathBuf;
use types::Config;

pub mod types;

pub fn get_config_path() -> PathBuf {
    match config_dir() {
        Some(base_dir) => base_dir.join("mcl/"),
        None => {
            tracing::error!("Could not determine config directory, falling back to ./config/");
            PathBuf::from("./config/")
        }
    }
}

fn ensure_config_exists(default_path: &str) -> PathBuf {
    let config_path = get_config_path().join("config.toml");

    if !config_path.exists() {
        if let Some(parent_dir) = config_path.parent() {
            match fs::create_dir_all(parent_dir) {
                Ok(_) => {}
                Err(e) => {
                    tracing::error!("Failed to create configuration directory: {}", e);
                }
            }
        }

        match fs::copy(default_path, &config_path) {
            Ok(_) => {
                tracing::debug!(
                    "Default configuration copied to '{}'",
                    config_path.display()
                );
            }
            Err(e) => {
                tracing::error!(
                    "Failed to copy default config from '{}' to '{}': {}",
                    default_path,
                    config_path.display(),
                    e
                );
            }
        }
    }

    config_path
}

pub fn load_config(config_path: &std::path::Path) -> Result<Config, ConfigError> {
    let built = match ConfigLoader::builder()
        .add_source(File::from(config_path).required(false))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(
                "Failed to build config from '{}': {}",
                config_path.display(),
                e
            );
            return Err(e);
        }
    };

    match built.try_deserialize() {
        Ok(config) => Ok(config),
        Err(e) => {
            tracing::error!(
                "Failed to deserialize config from '{}': {}",
                config_path.display(),
                e
            );
            Err(e)
        }
    }
}

pub static SETTINGS: Lazy<Config> = Lazy::new(|| {
    let path: PathBuf = ensure_config_exists("assets/config.toml");
    match load_config(&path) {
        Ok(config) => config,
        Err(e) => {
            tracing::error!("Config load failed, using defaults: {}", e);
            Config {
                general: types::General::default(),
                paths: types::Paths::default(),
                defaults: types::Defaults::default(),
                ui: types::Ui::default(),
            }
        }
    }
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_config_from_valid_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[general]
debug = true

[defaults]
memory_max = "4G"
"#,
        )
        .unwrap();
        let config = load_config(&path).unwrap();
        assert!(config.general.debug);
        assert_eq!(config.defaults.memory_max, "4G");
        assert_eq!(config.defaults.memory_min, "512M");
    }

    #[test]
    fn load_config_from_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        let config = load_config(&path).unwrap();
        assert!(!config.general.debug);
        assert_eq!(config.defaults.memory_max, "2G");
    }

    #[test]
    fn load_config_missing_file_uses_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nonexistent.toml");
        let config = load_config(&path).unwrap();
        assert!(!config.general.debug);
    }

    #[test]
    fn load_config_partial_sections() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
[paths]
instances_dir = "/custom/path"
"#,
        )
        .unwrap();
        let config = load_config(&path).unwrap();
        assert_eq!(config.paths.instances_dir, "/custom/path");
        assert!(config.paths.java_path.is_none());
    }
}
