// world save scanning. worlds are always directories (never zips) and store
// their icon as icon.png. also computes an approximate size from top-level
// files + region data so the user gets some sense of how chonky their world is.

use std::path::Path;

use super::mods::{ContentEntry, make_icon_pixels};

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

        let icon_bytes = std::fs::read(path.join("icon.png")).ok();
        let icon_lines = icon_bytes
            .as_ref()
            .and_then(|bytes| make_icon_pixels(bytes, 12, 6))
            .or_else(|| Some(super::mods::fallback_icon_large()));

        let description = world_description(&path);

        entries.push(ContentEntry {
            name: file_stem.clone(),
            file_stem,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_saves_dir(tmp: &Path, instance: &str) -> std::path::PathBuf {
        let dir = tmp.join(instance).join(".minecraft").join("saves");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_worlds_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        setup_saves_dir(tmp.path(), "inst");
        let worlds = scan_worlds(tmp.path(), "inst");
        assert!(worlds.is_empty());
    }

    #[test]
    fn scan_worlds_missing_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let worlds = scan_worlds(tmp.path(), "ghost");
        assert!(worlds.is_empty());
    }

    #[test]
    fn scan_worlds_finds_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_saves_dir(tmp.path(), "inst");
        std::fs::create_dir(dir.join("My World")).unwrap();
        std::fs::create_dir(dir.join("Creative")).unwrap();
        let worlds = scan_worlds(tmp.path(), "inst");
        assert_eq!(worlds.len(), 2);
    }

    #[test]
    fn scan_worlds_ignores_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_saves_dir(tmp.path(), "inst");
        std::fs::create_dir(dir.join("World1")).unwrap();
        std::fs::write(dir.join("stray-file.txt"), "not a world").unwrap();
        let worlds = scan_worlds(tmp.path(), "inst");
        assert_eq!(worlds.len(), 1);
    }

    #[test]
    fn scan_worlds_disabled_world() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_saves_dir(tmp.path(), "inst");
        std::fs::create_dir(dir.join("ActiveWorld")).unwrap();
        std::fs::create_dir(dir.join("HiddenWorld.disabled")).unwrap();
        let worlds = scan_worlds(tmp.path(), "inst");
        let active = worlds
            .iter()
            .find(|w| w.file_stem == "ActiveWorld")
            .unwrap();
        let hidden = worlds
            .iter()
            .find(|w| w.file_stem == "HiddenWorld")
            .unwrap();
        assert!(active.enabled);
        assert!(!hidden.enabled);
    }

    #[test]
    fn scan_worlds_sorted_case_insensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_saves_dir(tmp.path(), "inst");
        std::fs::create_dir(dir.join("Zeta")).unwrap();
        std::fs::create_dir(dir.join("alpha")).unwrap();
        std::fs::create_dir(dir.join("Beta")).unwrap();
        let worlds = scan_worlds(tmp.path(), "inst");
        let names: Vec<&str> = worlds.iter().map(|w| w.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "Beta", "Zeta"]);
    }
}
