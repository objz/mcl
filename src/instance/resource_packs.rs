use std::io::Read;
use std::path::Path;

use serde::Deserialize;

use super::mods::{make_icon_pixels, ModEntry};

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

pub fn scan_resource_packs(instances_dir: &Path, instance_name: &str) -> Vec<ModEntry> {
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
            if file_name.ends_with(".disabled") {
                (false, file_name.trim_end_matches(".disabled").to_string())
            } else {
                (true, file_name.clone())
            }
        } else if file_name.ends_with(".zip") {
            (true, file_name.trim_end_matches(".zip").to_string())
        } else if file_name.ends_with(".zip.disabled") {
            (
                false,
                file_name.trim_end_matches(".zip.disabled").to_string(),
            )
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

        entries.push(ModEntry {
            file_stem: file_stem.clone(),
            name: if name.is_empty() { file_stem } else { name },
            description,
            enabled,
            icon_bytes,
            path,
            icon_lines,
        });
    }

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

fn read_pack_metadata_from_zip(zip_path: &Path) -> (String, String, Option<Vec<u8>>) {
    let file = match std::fs::File::open(zip_path) {
        Ok(f) => f,
        Err(_) => return (String::new(), String::new(), None),
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return (String::new(), String::new(), None),
    };

    let description = read_pack_description(&mut archive);

    let icon_bytes = {
        let mut buf = Vec::new();
        match archive.by_name("pack.png") {
            Ok(mut entry) => {
                if entry.read_to_end(&mut buf).is_ok() {
                    Some(buf)
                } else {
                    None
                }
            }
            Err(_) => None,
        }
    };

    (String::new(), description, icon_bytes)
}

fn read_pack_description(archive: &mut zip::ZipArchive<std::fs::File>) -> String {
    let entry = match archive.by_name("pack.mcmeta") {
        Ok(e) => e,
        Err(_) => return String::new(),
    };

    let meta: PackMcMeta = match serde_json::from_reader(entry) {
        Ok(m) => m,
        Err(_) => return String::new(),
    };

    extract_description(&meta.pack.description)
}

fn read_pack_metadata_from_dir(dir: &Path) -> (String, String, Option<Vec<u8>>) {
    let actual_dir = if dir
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".disabled"))
    {
        dir.to_path_buf()
    } else {
        dir.to_path_buf()
    };

    let description = {
        let mcmeta_path = actual_dir.join("pack.mcmeta");
        match std::fs::read_to_string(&mcmeta_path) {
            Ok(content) => match serde_json::from_str::<PackMcMeta>(&content) {
                Ok(meta) => extract_description(&meta.pack.description),
                Err(_) => String::new(),
            },
            Err(_) => String::new(),
        }
    };

    let icon_bytes = {
        let icon_path = actual_dir.join("pack.png");
        std::fs::read(&icon_path).ok()
    };

    (String::new(), description, icon_bytes)
}
