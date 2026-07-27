// config loading: reads config.toml from the platform config dir, creates defaults if missing.
// everything lands in the SETTINGS static so the rest of the app can just grab it.

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
        .join("rmcl")
}

// seeds the config file from the bundled default on first run
fn ensure_config_exists() -> PathBuf {
    let config_path = get_config_path().join("config.toml");
    if !config_path.exists() {
        if let Some(parent) = config_path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            tracing::warn!(
                "Failed to create config directory {}: {}",
                parent.display(),
                e
            );
        }
        match fs::write(&config_path, include_str!("../../assets/config.toml")) {
            Ok(()) => tracing::debug!("Wrote default config to {}", config_path.display()),
            Err(e) => tracing::warn!(
                "Failed to write default config to {}: {}",
                config_path.display(),
                e
            ),
        }
    } else {
        tracing::trace!("Using existing config at {}", config_path.display());
    }
    config_path
}

pub fn load_config(config_path: &std::path::Path) -> Result<Config, ConfigError> {
    tracing::debug!("Loading config from {}", config_path.display());
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
            content: settings::Content::default(),
        }
    })
});

#[cfg(test)]
#[path = "tests/loading.rs"]
mod tests;
