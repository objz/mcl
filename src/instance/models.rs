// core data types for an instance: what loader it uses, what version, memory
// settings, etc. this is what gets persisted to instance.json

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    #[serde(default)]
    pub last_played: Option<DateTime<Utc>>,
    #[serde(default)]
    pub java_path: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_memory")]
    pub memory_max: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_memory")]
    pub memory_min: Option<String>,
    #[serde(default)]
    pub jvm_args: Vec<String>,
    #[serde(default)]
    pub resolution: Option<(u32, u32)>,
    #[serde(default)]
    pub config_sync_profile: Option<String>,
}

pub fn normalize_memory_value(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (digits, suffix) = match trimmed.chars().last() {
        Some(c) if c.is_ascii_alphabetic() => (&trimmed[..trimmed.len() - c.len_utf8()], Some(c)),
        Some(_) => (trimmed, None),
        None => return None,
    };

    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }

    let value = digits.parse::<u64>().ok()?;
    if value == 0 {
        return None;
    }

    match suffix.map(|c| c.to_ascii_uppercase()) {
        Some(unit @ ('K' | 'M' | 'G')) => Some(format!("{value}{unit}")),
        Some(_) => None,
        None => memory_number_to_string(value),
    }
}

fn deserialize_optional_memory<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(raw)) => Ok(normalize_memory_value(&raw)),
        Some(Value::Number(number)) => {
            let number = if let Some(value) = number.as_u64() {
                value
            } else if let Some(value) = number.as_i64() {
                let Ok(value) = u64::try_from(value) else {
                    return Ok(None);
                };
                value
            } else {
                let Some(value) = number.as_f64() else {
                    return Ok(None);
                };
                if !value.is_finite() || value.fract() != 0.0 || value < 0.0 {
                    return Ok(None);
                }
                value as u64
            };
            Ok(memory_number_to_string(number))
        }
        Some(_) => Ok(None),
    }
}

fn memory_number_to_string(value: u64) -> Option<String> {
    if value == 0 {
        return None;
    }
    if value < 128 {
        Some(format!("{value}G"))
    } else {
        Some(format!("{value}M"))
    }
}

#[cfg(test)]
#[path = "tests/models.rs"]
mod tests;
