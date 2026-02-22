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
        let active = worlds.iter().find(|w| w.file_stem == "ActiveWorld").unwrap();
        let hidden = worlds.iter().find(|w| w.file_stem == "HiddenWorld").unwrap();
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
