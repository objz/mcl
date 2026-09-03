// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// config loading: reads config.toml from the platform config dir, creates defaults if missing.
// everything lands in the SETTINGS static so the rest of the app can just grab it.

use config::{Config as ConfigLoader, ConfigError, File};
use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, RwLock, RwLockReadGuard};

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

pub struct ConfigStore(RwLock<Config>);

impl ConfigStore {
    pub fn read(&self) -> RwLockReadGuard<'_, Config> {
        self.0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn save_launcher_settings(&self, edited: Config) -> std::io::Result<()> {
        let path = get_config_path().join("config.toml");
        let current = self.read().clone();
        let mut persisted = load_config(&path).unwrap_or_else(|error| {
            tracing::warn!("Failed to merge config.toml while saving settings: {error}");
            current.clone()
        });
        persisted.defaults = edited.defaults.clone();
        persisted.paths.java_path = edited.paths.java_path.clone();
        let serialized = toml::to_string_pretty(&persisted).map_err(std::io::Error::other)?;
        crate::storage::write_atomic(&path, serialized.as_bytes())?;
        let mut runtime = current;
        runtime.defaults = edited.defaults;
        runtime.paths.java_path = edited.paths.java_path;
        *self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = runtime;
        Ok(())
    }

    pub fn reload(&self) -> Result<bool, ConfigError> {
        let mut config = load_config(&get_config_path().join("config.toml"))?;
        let restart_required = {
            let current = self.read();
            let changed = current.paths.instances_dir != config.paths.instances_dir
                || current.paths.meta_dir != config.paths.meta_dir
                || current.ui.image_protocol != config.ui.image_protocol;
            // App owns a manager and several watchers rooted at these paths.
            // Keep them stable for this process; persisted path edits apply on restart.
            config
                .paths
                .instances_dir
                .clone_from(&current.paths.instances_dir);
            config.paths.meta_dir.clone_from(&current.paths.meta_dir);
            config.ui.image_protocol = current.ui.image_protocol;
            changed
        };
        *self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = config;
        Ok(restart_required)
    }
}

pub static SETTINGS: LazyLock<ConfigStore> = LazyLock::new(|| {
    ConfigStore(RwLock::new({
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
    }))
});

#[cfg(test)]
#[path = "tests/loading.rs"]
mod tests;
