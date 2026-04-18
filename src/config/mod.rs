use config::{Config as ConfigLoader, ConfigError, File};
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

pub mod settings;
pub mod theme;

pub use settings::Config;

#[must_use]
pub fn get_config_path() -> PathBuf {
    dirs_next::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mcl")
}

fn ensure_config_exists() -> PathBuf {
    let config_path = get_config_path().join("config.toml");
    if !config_path.exists() {
        if let Some(parent) = config_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&config_path, include_str!("../../assets/config.toml"));
    }
    config_path
}

pub fn load_config(config_path: &std::path::Path) -> Result<Config, ConfigError> {
    ConfigLoader::builder()
        .add_source(File::from(config_path).required(false))
        .build()?
        .try_deserialize()
}

pub static SETTINGS: LazyLock<Config> = LazyLock::new(|| {
    let path = ensure_config_exists();
    load_config(&path).unwrap_or_else(|e| {
        tracing::error!("Config load failed, using defaults: {}", e);
        Config {
            general: settings::General::default(),
            paths: settings::Paths::default(),
            defaults: settings::Defaults::default(),
            ui: settings::Ui::default(),
        }
    })
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
            [defaults]
            memory_max = "4G"
            "#,
        )
        .unwrap();
        let config = load_config(&path).unwrap();
        assert_eq!(config.defaults.memory_max, "4G");
        assert_eq!(config.defaults.memory_min, "512M");
    }

    #[test]
    fn load_config_from_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        let config = load_config(&path).unwrap();
        assert_eq!(config.defaults.memory_max, "2G");
    }

    #[test]
    fn load_config_missing_file_uses_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nonexistent.toml");
        load_config(&path).unwrap();
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
