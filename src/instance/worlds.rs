use std::path::Path;

use super::mods::{make_icon_pixels, ModEntry};

pub fn scan_worlds(instances_dir: &Path, instance_name: &str) -> Vec<ModEntry> {
    let saves_dir = instances_dir
        .join(instance_name)
        .join(".minecraft")
        .join("saves");

    let read_dir = match std::fs::read_dir(&saves_dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        let (enabled, file_stem) = if file_name.ends_with(".disabled") {
            (false, file_name.trim_end_matches(".disabled").to_string())
        } else {
            (true, file_name.clone())
        };

        let icon_bytes = std::fs::read(path.join("icon.png")).ok();
        let icon_lines = icon_bytes
            .as_ref()
            .and_then(|bytes| make_icon_pixels(bytes, 12, 6));

        entries.push(ModEntry {
            file_stem: file_stem.clone(),
            name: file_stem,
            description: String::new(),
            enabled,
            icon_bytes,
            path,
            icon_lines,
        });
    }

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}
