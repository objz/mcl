// shader pack scanning. structurally almost identical to resource packs
// (zip or dir, pack.mcmeta for metadata, pack.png for icon)

use std::path::Path;

use super::mods::{ContentEntry, make_icon_pixels};
use super::resource_packs::{PackMcMeta, extract_description};

pub fn scan_one_shader(path: &Path, file_stem: &str, enabled: bool) -> ContentEntry {
    let is_dir = path.is_dir();
    let (description, icon_bytes) = if is_dir {
        read_shader_metadata_from_dir(path)
    } else {
        read_shader_metadata_from_zip(path)
    };

    let icon_lines = icon_bytes
        .as_ref()
        .and_then(|bytes| make_icon_pixels(bytes, 6, 3));

    ContentEntry {
        file_stem: file_stem.to_owned(),
        name: file_stem.to_owned(),
        description,
        enabled,
        icon_bytes,
        path: path.to_path_buf(),
        icon_lines,
    }
}

pub fn scan_shaders(instances_dir: &Path, instance_name: &str) -> Vec<ContentEntry> {
    let shaders_dir = instances_dir
        .join(instance_name)
        .join(".minecraft")
        .join("shaderpacks");

    let read_dir = match std::fs::read_dir(&shaders_dir) {
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

        entries.push(scan_one_shader(&path, &file_stem, enabled));
    }

    entries.sort_by_cached_key(|e| e.name.to_lowercase());
    entries
}

fn read_shader_metadata_from_zip(zip_path: &Path) -> (String, Option<Vec<u8>>) {
    let Some(mut archive) = super::open_zip(zip_path) else {
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

fn read_shader_metadata_from_dir(dir: &Path) -> (String, Option<Vec<u8>>) {
    let description = std::fs::read_to_string(dir.join("pack.mcmeta"))
        .ok()
        .and_then(|content| serde_json::from_str::<PackMcMeta>(&content).ok())
        .map(|meta| extract_description(&meta.pack.description))
        .unwrap_or_default();

    let icon_bytes = std::fs::read(dir.join("pack.png")).ok();

    (description, icon_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_shaders_dir(tmp: &std::path::Path, instance: &str) -> std::path::PathBuf {
        let dir = tmp.join(instance).join(".minecraft").join("shaderpacks");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_shaders_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        setup_shaders_dir(tmp.path(), "inst");
        let shaders = scan_shaders(tmp.path(), "inst");
        assert!(shaders.is_empty());
    }

    #[test]
    fn scan_shaders_missing_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let shaders = scan_shaders(tmp.path(), "ghost");
        assert!(shaders.is_empty());
    }

    #[test]
    fn scan_shaders_finds_zip_and_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_shaders_dir(tmp.path(), "inst");
        std::fs::write(dir.join("shader-a.zip"), b"PK\x03\x04").unwrap();
        std::fs::create_dir(dir.join("shader-b")).unwrap();
        let shaders = scan_shaders(tmp.path(), "inst");
        assert_eq!(shaders.len(), 2);
    }

    #[test]
    fn scan_shaders_disabled_variants() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_shaders_dir(tmp.path(), "inst");
        std::fs::write(dir.join("active.zip"), b"PK\x03\x04").unwrap();
        std::fs::write(dir.join("off.zip.disabled"), b"PK\x03\x04").unwrap();
        std::fs::create_dir(dir.join("dirshader.disabled")).unwrap();
        let shaders = scan_shaders(tmp.path(), "inst");
        let active = shaders.iter().find(|s| s.file_stem == "active").unwrap();
        let off = shaders.iter().find(|s| s.file_stem == "off").unwrap();
        let diroff = shaders.iter().find(|s| s.file_stem == "dirshader").unwrap();
        assert!(active.enabled);
        assert!(!off.enabled);
        assert!(!diroff.enabled);
    }

    #[test]
    fn scan_shaders_ignores_non_shader_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_shaders_dir(tmp.path(), "inst");
        std::fs::write(dir.join("readme.txt"), "not a shader").unwrap();
        std::fs::write(dir.join("valid.zip"), b"PK\x03\x04").unwrap();
        let shaders = scan_shaders(tmp.path(), "inst");
        assert_eq!(shaders.len(), 1);
    }
}
