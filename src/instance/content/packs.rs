// shared pack.mcmeta scanning for resource packs and shader packs.

use std::path::Path;

use serde::Deserialize;

use super::entry::ContentEntry;
use super::{fallback_icon, make_icon_pixels};

#[derive(Deserialize, Default)]
struct PackMcMeta {
    #[serde(default)]
    pack: PackInfo,
}

#[derive(Deserialize, Default)]
struct PackInfo {
    #[serde(default)]
    description: serde_json::Value,
}

pub(crate) fn extract_description(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Object(object) => object
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_owned(),
        serde_json::Value::Array(values) => values
            .iter()
            .filter_map(|value| match value {
                serde_json::Value::String(text) => Some(text.as_str()),
                serde_json::Value::Object(object) => {
                    object.get("text").and_then(|value| value.as_str())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

pub(crate) fn scan_one_pack(path: &Path, file_stem: &str, enabled: bool) -> ContentEntry {
    let (description, icon_bytes) = if path.is_dir() {
        read_metadata_from_dir(path)
    } else {
        read_metadata_from_zip(path)
    };
    let icon_lines = icon_bytes
        .as_ref()
        .and_then(|bytes| make_icon_pixels(bytes, 6, 3))
        .or_else(|| Some(fallback_icon()));

    ContentEntry {
        file_stem: file_stem.to_owned(),
        name: file_stem.to_owned(),
        source_slug: None,
        installed_path: None,
        provider_project: None,
        world_details: None,
        title_suffix: None,
        footer_label: None,
        footer_change: None,
        description,
        enabled,
        icon_bytes,
        provider_icon: false,
        provider_description: false,
        path: path.to_path_buf(),
        icon_lines,
    }
}

pub(crate) fn scan_packs(
    instances_dir: &Path,
    instance_name: &str,
    directory: &str,
) -> Vec<ContentEntry> {
    let directory = instances_dir
        .join(instance_name)
        .join(crate::storage::MINECRAFT_DIR_NAME)
        .join(directory);
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut packs = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let file_name = path.file_name()?.to_str()?;
            let (enabled, file_stem) = if path.is_dir() {
                super::parse_enabled_stem_dir(file_name)
            } else {
                super::parse_enabled_stem(file_name, ".zip")?
            };
            Some(scan_one_pack(&path, &file_stem, enabled))
        })
        .collect::<Vec<_>>();
    packs.sort_by_cached_key(|entry| entry.name.to_lowercase());
    packs
}

fn read_metadata_from_zip(path: &Path) -> (String, Option<Vec<u8>>) {
    let Some(mut archive) = super::open_zip(path) else {
        return (String::new(), None);
    };
    let description = archive
        .by_name("pack.mcmeta")
        .ok()
        .and_then(|entry| serde_json::from_reader::<_, PackMcMeta>(entry).ok())
        .map(|meta| extract_description(&meta.pack.description))
        .unwrap_or_default();
    let icon_bytes = super::read_icon_from_zip(&mut archive);
    (description, icon_bytes)
}

fn read_metadata_from_dir(path: &Path) -> (String, Option<Vec<u8>>) {
    let description = std::fs::read_to_string(path.join("pack.mcmeta"))
        .ok()
        .and_then(|content| serde_json::from_str::<PackMcMeta>(&content).ok())
        .map(|meta| extract_description(&meta.pack.description))
        .unwrap_or_default();
    (description, std::fs::read(path.join("pack.png")).ok())
}
