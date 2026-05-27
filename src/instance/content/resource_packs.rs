// resource pack scanning. packs can be either .zip files or plain directories,
// and metadata lives in pack.mcmeta (which mojang decided should have like 3
// different ways to encode a description, because why not)

use std::path::Path;

use serde::Deserialize;

use super::mods::{ContentEntry, make_icon_pixels};

#[derive(Deserialize, Default)]
pub(crate) struct PackMcMeta {
    #[serde(default)]
    pub pack: PackInfo,
}

#[derive(Deserialize, Default)]
pub(crate) struct PackInfo {
    #[serde(default)]
    pub description: serde_json::Value,
}

// description can be a plain string, a chat component object with "text",
// or an array mixing both. thanks mojang, very cool.
pub(crate) fn extract_description(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(obj) => obj
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|v| match v {
                serde_json::Value::String(s) => Some(s.as_str()),
                serde_json::Value::Object(obj) => obj.get("text").and_then(|v| v.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

pub fn scan_one_resource_pack(path: &Path, file_stem: &str, enabled: bool) -> ContentEntry {
    let is_dir = path.is_dir();
    let (name, description, icon_bytes) = if is_dir {
        read_pack_metadata_from_dir(path)
    } else {
        read_pack_metadata_from_zip(path)
    };

    let icon_lines = icon_bytes
        .as_ref()
        .and_then(|bytes| make_icon_pixels(bytes, 6, 3))
        .or_else(|| Some(super::mods::fallback_icon()));

    let display_name = if name.is_empty() {
        file_stem.to_owned()
    } else {
        name
    };

    ContentEntry {
        file_stem: file_stem.to_owned(),
        name: display_name,
        description,
        enabled,
        icon_bytes,
        path: path.to_path_buf(),
        icon_lines,
    }
}

pub fn scan_resource_packs(instances_dir: &Path, instance_name: &str) -> Vec<ContentEntry> {
    let packs_dir = instances_dir
        .join(instance_name)
        .join(".minecraft")
        .join("resourcepacks");

    let read_dir = match std::fs::read_dir(&packs_dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();

    for entry in read_dir.flatten() {
        let path = entry.path();
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        let (enabled, file_stem) = if path.is_dir() {
            super::parse_enabled_stem_dir(&file_name)
        } else if let Some(pair) = super::parse_enabled_stem(&file_name, ".zip") {
            pair
        } else {
            continue;
        };

        entries.push(scan_one_resource_pack(&path, &file_stem, enabled));
    }

    entries.sort_by_cached_key(|e| e.name.to_lowercase());
    entries
}

fn read_pack_metadata_from_zip(zip_path: &Path) -> (String, String, Option<Vec<u8>>) {
    let Some(mut archive) = super::open_zip(zip_path) else {
        return (String::new(), String::new(), None);
    };
    let description = read_pack_description(&mut archive);
    let icon_bytes = super::read_icon_from_zip(&mut archive);
    (String::new(), description, icon_bytes)
}

fn read_pack_description(archive: &mut zip::ZipArchive<std::fs::File>) -> String {
    archive
        .by_name("pack.mcmeta")
        .ok()
        .and_then(|entry| serde_json::from_reader::<_, PackMcMeta>(entry).ok())
        .map(|meta| extract_description(&meta.pack.description))
        .unwrap_or_default()
}

fn read_pack_metadata_from_dir(dir: &Path) -> (String, String, Option<Vec<u8>>) {
    let description = std::fs::read_to_string(dir.join("pack.mcmeta"))
        .ok()
        .and_then(|content| serde_json::from_str::<PackMcMeta>(&content).ok())
        .map(|meta| extract_description(&meta.pack.description))
        .unwrap_or_default();

    let icon_bytes = std::fs::read(dir.join("pack.png")).ok();

    (String::new(), description, icon_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // every case exercises a distinct match arm in extract_description.
    // string + object{text} + object{no text} + array(string) +
    // array(object) + array(mixed) + array(empty) + null + number + bool.
    // mutating any arm to fall through to "" would fail at least one case.
    #[rstest::rstest]
    #[case::string(json!("Simple pack"), "Simple pack")]
    #[case::object_with_text(json!({"text": "Hello world"}), "Hello world")]
    #[case::object_without_text(json!({"color": "red"}), "")]
    #[case::array_of_strings(json!(["Hello", " ", "world"]), "Hello world")]
    #[case::array_of_objects(json!([{"text": "A"}, {"text": "B"}]), "AB")]
    #[case::mixed_array(json!(["Prefix ", {"text": "suffix"}]), "Prefix suffix")]
    #[case::empty_array(json!([]), "")]
    #[case::null(serde_json::Value::Null, "")]
    #[case::number(json!(42), "")]
    #[case::bool(json!(true), "")]
    fn extract_description_handles(#[case] input: serde_json::Value, #[case] expected: &str) {
        assert_eq!(extract_description(&input), expected);
    }
}
