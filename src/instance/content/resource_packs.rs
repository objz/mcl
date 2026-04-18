use std::path::Path;

use serde::Deserialize;

use super::mods::{make_icon_pixels, ContentEntry};

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

        let is_dir = path.is_dir();
        let (enabled, file_stem) = if is_dir {
            super::parse_enabled_stem_dir(&file_name)
        } else if let Some(pair) = super::parse_enabled_stem(&file_name, ".zip") {
            pair
        } else {
            continue;
        };

        let (name, description, icon_bytes) = if is_dir {
            read_pack_metadata_from_dir(&path)
        } else {
            read_pack_metadata_from_zip(&path)
        };

        let icon_lines = icon_bytes
            .as_ref()
            .and_then(|bytes| make_icon_pixels(bytes, 6, 3));

        let display_name = if name.is_empty() { file_stem.clone() } else { name };
        entries.push(ContentEntry {
            file_stem,
            name: display_name,
            description,
            enabled,
            icon_bytes,
            path,
            icon_lines,
        });
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

    #[test]
    fn extract_description_from_string() {
        let val = serde_json::json!("Simple pack");
        assert_eq!(extract_description(&val), "Simple pack");
    }

    #[test]
    fn extract_description_from_object_with_text() {
        let val = serde_json::json!({"text": "Hello world"});
        assert_eq!(extract_description(&val), "Hello world");
    }

    #[test]
    fn extract_description_from_object_without_text() {
        let val = serde_json::json!({"color": "red"});
        assert_eq!(extract_description(&val), "");
    }

    #[test]
    fn extract_description_from_array_of_strings() {
        let val = serde_json::json!(["Hello", " ", "world"]);
        assert_eq!(extract_description(&val), "Hello world");
    }

    #[test]
    fn extract_description_from_array_of_objects() {
        let val = serde_json::json!([{"text": "A"}, {"text": "B"}]);
        assert_eq!(extract_description(&val), "AB");
    }

    #[test]
    fn extract_description_from_mixed_array() {
        let val = serde_json::json!(["Prefix ", {"text": "suffix"}]);
        assert_eq!(extract_description(&val), "Prefix suffix");
    }

    #[test]
    fn extract_description_from_empty_array() {
        let val = serde_json::json!([]);
        assert_eq!(extract_description(&val), "");
    }

    #[test]
    fn extract_description_from_null() {
        let val = serde_json::Value::Null;
        assert_eq!(extract_description(&val), "");
    }

    #[test]
    fn extract_description_from_number() {
        let val = serde_json::json!(42);
        assert_eq!(extract_description(&val), "");
    }

    #[test]
    fn extract_description_from_bool() {
        let val = serde_json::json!(true);
        assert_eq!(extract_description(&val), "");
    }

    fn setup_packs_dir(tmp: &std::path::Path, instance: &str) -> std::path::PathBuf {
        let dir = tmp
            .join(instance)
            .join(".minecraft")
            .join("resourcepacks");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_resource_packs_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        setup_packs_dir(tmp.path(), "inst");
        let packs = scan_resource_packs(tmp.path(), "inst");
        assert!(packs.is_empty());
    }

    #[test]
    fn scan_resource_packs_missing_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let packs = scan_resource_packs(tmp.path(), "ghost");
        assert!(packs.is_empty());
    }

    #[test]
    fn scan_resource_packs_finds_zips_and_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_packs_dir(tmp.path(), "inst");
        std::fs::write(dir.join("pack-a.zip"), b"PK\x03\x04").unwrap();
        std::fs::create_dir(dir.join("pack-b")).unwrap();
        let packs = scan_resource_packs(tmp.path(), "inst");
        assert_eq!(packs.len(), 2);
    }

    #[test]
    fn scan_resource_packs_disabled_variants() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_packs_dir(tmp.path(), "inst");
        std::fs::write(dir.join("on.zip"), b"PK\x03\x04").unwrap();
        std::fs::write(dir.join("off.zip.disabled"), b"PK\x03\x04").unwrap();
        std::fs::create_dir(dir.join("diron")).unwrap();
        std::fs::create_dir(dir.join("diroff.disabled")).unwrap();
        let packs = scan_resource_packs(tmp.path(), "inst");
        assert_eq!(packs.len(), 4);
        let on_zip = packs.iter().find(|p| p.file_stem == "on").unwrap();
        let off_zip = packs.iter().find(|p| p.file_stem == "off").unwrap();
        let on_dir = packs.iter().find(|p| p.file_stem == "diron").unwrap();
        let off_dir = packs.iter().find(|p| p.file_stem == "diroff").unwrap();
        assert!(on_zip.enabled);
        assert!(!off_zip.enabled);
        assert!(on_dir.enabled);
        assert!(!off_dir.enabled);
    }

    #[test]
    fn scan_resource_packs_ignores_non_pack_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_packs_dir(tmp.path(), "inst");
        std::fs::write(dir.join("notes.txt"), "not a pack").unwrap();
        std::fs::write(dir.join("valid.zip"), b"PK\x03\x04").unwrap();
        let packs = scan_resource_packs(tmp.path(), "inst");
        assert_eq!(packs.len(), 1);
    }
}
