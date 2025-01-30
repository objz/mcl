use config::{Config as ConfigLoader, ConfigError, File};
use dirs_next::config_dir;
use std::fs;
use std::path::PathBuf;
use obj::Config;

pub mod obj;

fn get_config_path() -> PathBuf {
    let base_dir = config_dir().unwrap();
    base_dir.join("mcl/config.toml")
}

pub fn ensure_config_exists(default_path: &str) -> PathBuf {
    let config_path = get_config_path();

    if !config_path.exists() {
        if let Some(parent_dir) = config_path.parent() {
            fs::create_dir_all(parent_dir).expect("Failed to create configuration directory");
        }

        fs::copy(default_path, &config_path).expect("Failed to copy default configuration file");
        println!("Default configuration copied to '{}'", config_path.display());
    }

    config_path
}

pub fn load_config(config_path: &PathBuf) -> Result<Config, ConfigError> {
    ConfigLoader::builder()
        .add_source(File::from(config_path.clone()))
        .build()?
        .try_deserialize()
}
