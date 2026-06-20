// world save scanning. worlds are always directories (never zips) and store
// their icon as icon.png. also computes an approximate size from top-level
// files + region data so the user gets some sense of how chonky their world is.

use std::path::Path;

use super::mods::{ContentEntry, make_icon_pixels};

pub fn scan_one_world(path: &Path, file_stem: &str, enabled: bool) -> ContentEntry {
    let icon_bytes = std::fs::read(path.join("icon.png")).ok();
    let icon_lines = icon_bytes
        .as_ref()
        .and_then(|bytes| make_icon_pixels(bytes, 12, 6))
        .or_else(|| Some(super::mods::fallback_icon_large()));

    let description = world_description(path);

    ContentEntry {
        name: file_stem.to_owned(),
        file_stem: file_stem.to_owned(),
        description,
        enabled,
        icon_bytes,
        path: path.to_path_buf(),
        icon_lines,
    }
}

pub fn scan_worlds(instances_dir: &Path, instance_name: &str) -> Vec<ContentEntry> {
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

        let (enabled, file_stem) = super::parse_enabled_stem_dir(&file_name);
        entries.push(scan_one_world(&path, &file_stem, enabled));
    }

    entries.sort_by_cached_key(|e| e.name.to_lowercase());
    entries
}

fn world_description(world_dir: &Path) -> String {
    let level_dat = world_dir.join("level.dat");

    let created = world_dir
        .metadata()
        .ok()
        .and_then(|m| m.created().ok().or_else(|| m.modified().ok()))
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    let modified = level_dat
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    let dir_size = dir_size_approx(world_dir);

    let mut lines = Vec::new();

    if let Some(secs) = created
        && let Some(dt) = chrono::DateTime::from_timestamp(secs as i64, 0)
    {
        lines.push(format!("Created:  {}", dt.format("%Y-%m-%d %H:%M")));
    }

    if let Some(secs) = modified
        && let Some(dt) = chrono::DateTime::from_timestamp(secs as i64, 0)
    {
        lines.push(format!("Played:   {}", dt.format("%Y-%m-%d %H:%M")));
    }

    if dir_size > 0 {
        lines.push(format!("Size:     {}", format_size(dir_size)));
    }

    lines.join("\n")
}

// only counts top-level files + region/ contents, not a full recursive walk.
// good enough for a quick size estimate without blocking the UI on huge worlds.
fn dir_size_approx(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(rd) = std::fs::read_dir(path) {
        for entry in rd.flatten() {
            if let Ok(meta) = entry.metadata()
                && meta.is_file()
            {
                total += meta.len();
            }
        }
    }
    // Check region folder too (main chunk data)
    let region = path.join("region");
    if let Ok(rd) = std::fs::read_dir(region) {
        for entry in rd.flatten() {
            if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
