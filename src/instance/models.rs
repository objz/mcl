// core data types for an instance: what loader it uses, what version, memory
// settings, etc. this is what gets persisted to instance.json

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModLoader {
    Vanilla,
    Fabric,
    Forge,
    NeoForge,
    Quilt,
}

impl fmt::Display for ModLoader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModLoader::Vanilla => write!(f, "Vanilla"),
            ModLoader::Fabric => write!(f, "Fabric"),
            ModLoader::Forge => write!(f, "Forge"),
            ModLoader::NeoForge => write!(f, "NeoForge"),
            ModLoader::Quilt => write!(f, "Quilt"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceConfig {
    pub name: String,
    pub game_version: String,
    pub loader: ModLoader,
    pub loader_version: Option<String>,
    pub created: DateTime<Utc>,
    pub last_played: Option<DateTime<Utc>>,
    pub java_path: Option<String>,
    pub memory_max: Option<String>,
    pub memory_min: Option<String>,
    pub jvm_args: Vec<String>,
    pub resolution: Option<(u32, u32)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_config_roundtrip() {
        let config = InstanceConfig {
            name: "test".to_string(),
            game_version: "1.20.1".to_string(),
            loader: ModLoader::Fabric,
            loader_version: Some("0.15.0".to_string()),
            created: Utc::now(),
            last_played: None,
            java_path: None,
            memory_max: Some("4G".to_string()),
            memory_min: Some("512M".to_string()),
            jvm_args: vec![],
            resolution: Some((1920, 1080)),
        };
        let json = serde_json::to_string_pretty(&config).expect("serialize");
        let parsed: InstanceConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.name, config.name);
        assert_eq!(parsed.game_version, config.game_version);
        assert_eq!(parsed.loader, config.loader);
        assert_eq!(parsed.resolution, config.resolution);
    }
}
