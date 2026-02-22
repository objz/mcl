use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;

type IconCell = (u8, u8, u8, u8, u8, u8);

#[derive(Debug, Clone)]
pub struct ModEntry {
    pub file_stem: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub icon_bytes: Option<Vec<u8>>,
    pub path: PathBuf,
    pub icon_lines: Option<Vec<Vec<IconCell>>>,
}

#[derive(Deserialize, Default)]
struct FabricModJson {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub icon: String,
}

pub fn scan_mods(instances_dir: &Path, instance_name: &str) -> Vec<ModEntry> {
    let mods_dir = instances_dir
        .join(instance_name)
        .join(".minecraft")
        .join("mods");

    let read_dir = match std::fs::read_dir(&mods_dir) {
        Ok(read_dir) => read_dir,
        Err(_) => return Vec::new(),
    };

    let mut entries = Vec::new();

    for entry in read_dir.flatten() {
        let path = entry.path();
        let file_name = match path.file_name().and_then(|name| name.to_str()) {
            Some(name) => name.to_string(),
            None => continue,
        };

        let (enabled, file_stem) = if file_name.ends_with(".jar") {
            (true, file_name.trim_end_matches(".jar").to_string())
        } else if file_name.ends_with(".jar.disabled") {
            (
                false,
                file_name.trim_end_matches(".jar.disabled").to_string(),
            )
        } else {
            continue;
        };

        let (name, description, icon_bytes) = read_mod_metadata(&path);
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

    entries.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    entries
}

fn read_mod_metadata(jar_path: &Path) -> (String, String, Option<Vec<u8>>) {
    let file = match std::fs::File::open(jar_path) {
        Ok(file) => file,
        Err(_) => return (String::new(), String::new(), None),
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(_) => return (String::new(), String::new(), None),
    };

    if let Some((name, description, icon_path)) = read_fabric_meta(&mut archive) {
        let icon_bytes = if icon_path.is_empty() {
            None
        } else {
            read_zip_bytes(&mut archive, &icon_path)
        };
        return (name, description, icon_bytes);
    }

    let icon_bytes = read_zip_bytes(&mut archive, "pack.png");
    (String::new(), String::new(), icon_bytes)
}

fn read_fabric_meta(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Option<(String, String, String)> {
    let entry = archive.by_name("fabric.mod.json").ok()?;
    let data: FabricModJson = serde_json::from_reader(entry).ok()?;
    Some((data.name, data.description, data.icon))
}

fn read_zip_bytes(archive: &mut zip::ZipArchive<std::fs::File>, path: &str) -> Option<Vec<u8>> {
    let mut entry = archive.by_name(path).ok()?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

pub(crate) fn make_icon_pixels(
    bytes: &[u8],
    width: u16,
    height: u16,
) -> Option<Vec<Vec<IconCell>>> {
    let img = image::load_from_memory(bytes).ok()?;
    let resized = img.resize_exact(
        u32::from(width),
        u32::from(height) * 2,
        image::imageops::FilterType::Nearest,
    );
    let rgb = resized.to_rgb8();

    let mut rows = Vec::new();
    for row in 0..height {
        let mut cols = Vec::new();
        for col in 0..width {
            let top_y = u32::from(row) * 2;
            let bottom_y = (u32::from(row) * 2 + 1).min(rgb.height().saturating_sub(1));
            let [tr, tg, tb] = rgb.get_pixel(u32::from(col), top_y).0;
            let [br, bg, bb] = rgb.get_pixel(u32::from(col), bottom_y).0;
            cols.push((br, bg, bb, tr, tg, tb));
        }
        rows.push(cols);
    }

    Some(rows)
}

pub fn toggle_mod(entry: &ModEntry) -> Result<(), std::io::Error> {
    let file_name = match entry.path.file_name().and_then(|name| name.to_str()) {
        Some(name) => name,
        None => return Ok(()),
    };

    let new_name = if entry.enabled {
        format!("{file_name}.disabled")
    } else {
        file_name.trim_end_matches(".disabled").to_string()
    };

    let mut new_path = entry.path.clone();
    new_path.set_file_name(new_name);
    std::fs::rename(&entry.path, new_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_mods_dir(tmp: &Path, instance: &str) -> PathBuf {
        let dir = tmp.join(instance).join(".minecraft").join("mods");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_mods_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        setup_mods_dir(tmp.path(), "inst");
        let mods = scan_mods(tmp.path(), "inst");
        assert!(mods.is_empty());
    }

    #[test]
    fn scan_mods_missing_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let mods = scan_mods(tmp.path(), "ghost");
        assert!(mods.is_empty());
    }

    #[test]
    fn scan_mods_finds_jar_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_mods_dir(tmp.path(), "inst");
        std::fs::write(dir.join("cool-mod.jar"), b"PK\x03\x04").unwrap();
        std::fs::write(dir.join("other-mod.jar.disabled"), b"PK\x03\x04").unwrap();
        let mods = scan_mods(tmp.path(), "inst");
        assert_eq!(mods.len(), 2);
    }

    #[test]
    fn scan_mods_enabled_disabled_flags() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_mods_dir(tmp.path(), "inst");
        std::fs::write(dir.join("active.jar"), b"PK\x03\x04").unwrap();
        std::fs::write(dir.join("inactive.jar.disabled"), b"PK\x03\x04").unwrap();
        let mods = scan_mods(tmp.path(), "inst");
        let active = mods.iter().find(|m| m.file_stem == "active").unwrap();
        let inactive = mods.iter().find(|m| m.file_stem == "inactive").unwrap();
        assert!(active.enabled);
        assert!(!inactive.enabled);
    }

    #[test]
    fn scan_mods_ignores_non_jar() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_mods_dir(tmp.path(), "inst");
        std::fs::write(dir.join("readme.txt"), "not a mod").unwrap();
        std::fs::write(dir.join("config.json"), "{}").unwrap();
        std::fs::write(dir.join("real.jar"), b"PK\x03\x04").unwrap();
        let mods = scan_mods(tmp.path(), "inst");
        assert_eq!(mods.len(), 1);
    }

    #[test]
    fn scan_mods_sorted_case_insensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_mods_dir(tmp.path(), "inst");
        std::fs::write(dir.join("Zebra.jar"), b"PK\x03\x04").unwrap();
        std::fs::write(dir.join("alpha.jar"), b"PK\x03\x04").unwrap();
        std::fs::write(dir.join("Beta.jar"), b"PK\x03\x04").unwrap();
        let mods = scan_mods(tmp.path(), "inst");
        let names: Vec<&str> = mods.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "Beta", "Zebra"]);
    }

    #[test]
    fn toggle_mod_enable() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_mods_dir(tmp.path(), "inst");
        let disabled_path = dir.join("mymod.jar.disabled");
        std::fs::write(&disabled_path, b"PK\x03\x04").unwrap();

        let entry = ModEntry {
            file_stem: "mymod".to_string(),
            name: "mymod".to_string(),
            description: String::new(),
            enabled: false,
            icon_bytes: None,
            path: disabled_path.clone(),
            icon_lines: None,
        };

        toggle_mod(&entry).unwrap();
        assert!(!disabled_path.exists());
        assert!(dir.join("mymod.jar").exists());
    }

    #[test]
    fn toggle_mod_disable() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_mods_dir(tmp.path(), "inst");
        let enabled_path = dir.join("mymod.jar");
        std::fs::write(&enabled_path, b"PK\x03\x04").unwrap();

        let entry = ModEntry {
            file_stem: "mymod".to_string(),
            name: "mymod".to_string(),
            description: String::new(),
            enabled: true,
            icon_bytes: None,
            path: enabled_path.clone(),
            icon_lines: None,
        };

        toggle_mod(&entry).unwrap();
        assert!(!enabled_path.exists());
        assert!(dir.join("mymod.jar.disabled").exists());
    }
}
