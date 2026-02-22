use std::io::Read;
use std::path::Path;

use super::mods::{make_icon_pixels, ModEntry};
use super::resource_packs::{extract_description, PackMcMeta};

pub fn scan_shaders(instances_dir: &Path, instance_name: &str) -> Vec<ModEntry> {
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

        let (description, icon_bytes) = if is_dir {
            read_shader_metadata_from_dir(&path)
        } else {
            read_shader_metadata_from_zip(&path)
        };

        let icon_lines = icon_bytes
            .as_ref()
            .and_then(|bytes| make_icon_pixels(bytes, 6, 3));

        entries.push(ModEntry {
            file_stem: file_stem.clone(),
            name: file_stem,
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

fn read_shader_metadata_from_zip(zip_path: &Path) -> (String, Option<Vec<u8>>) {
    let file = match std::fs::File::open(zip_path) {
        Ok(f) => f,
        Err(_) => return (String::new(), None),
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return (String::new(), None),
    };

    let description = match archive.by_name("pack.mcmeta") {
        Ok(entry) => match serde_json::from_reader::<_, PackMcMeta>(entry) {
            Ok(meta) => extract_description(&meta.pack.description),
            Err(_) => String::new(),
        },
        Err(_) => String::new(),
    };

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

    (description, icon_bytes)
}

fn read_shader_metadata_from_dir(dir: &Path) -> (String, Option<Vec<u8>>) {
    let description = match std::fs::read_to_string(dir.join("pack.mcmeta")) {
        Ok(content) => match serde_json::from_str::<PackMcMeta>(&content) {
            Ok(meta) => extract_description(&meta.pack.description),
            Err(_) => String::new(),
        },
        Err(_) => String::new(),
    };

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
