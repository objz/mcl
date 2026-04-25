// mod scanning, metadata extraction, and icon rendering for the content list.
// jar files are just zips, so it cracks them open looking for loader-specific
// metadata (fabric.mod.json, quilt.mod.json, mods.toml, mcmod.info) to get
// names, descriptions, and icons. if none of those work, falls back to common
// root-level icon paths (logo.png, icon.png, pack.png) or just the filename.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Deserialize;

// a single "pixel" in the terminal icon. uses the unicode half-block trick
// where each character cell shows two vertical pixels (fg = top, bg = bottom)
#[derive(Debug, Clone, Copy)]
pub struct IconCell {
    pub bg_r: u8,
    pub bg_g: u8,
    pub bg_b: u8,
    pub fg_r: u8,
    pub fg_g: u8,
    pub fg_b: u8,
}

#[derive(Debug, Clone)]
pub struct ContentEntry {
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
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    icon: serde_json::Value,
}

impl FabricModJson {
    fn icon_path(&self) -> String {
        icon_path_from_value(&self.icon)
    }
}

// fabric and quilt both support icon as a string path or a map of
// resolution -> path. if it's a map, just grab the first one.
fn icon_path_from_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(map) => map
            .values()
            .find_map(|v| v.as_str())
            .unwrap_or("")
            .to_owned(),
        _ => String::new(),
    }
}

// quilt puts its metadata under a "metadata" sub-object
#[derive(Deserialize, Default)]
struct QuiltModJson {
    #[serde(default)]
    quilt_loader: QuiltLoader,
}

#[derive(Deserialize, Default)]
struct QuiltLoader {
    #[serde(default)]
    metadata: QuiltMetadata,
}

#[derive(Deserialize, Default)]
struct QuiltMetadata {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    icon: serde_json::Value,
}

impl QuiltMetadata {
    fn icon_path(&self) -> String {
        icon_path_from_value(&self.icon)
    }
}

pub fn scan_one_mod(path: &Path, file_stem: &str, enabled: bool) -> ContentEntry {
    let (name, description, icon_bytes) = read_mod_metadata(path);
    let icon_lines = icon_bytes
        .as_ref()
        .and_then(|bytes| make_icon_pixels(bytes, 6, 3))
        .or_else(|| Some(fallback_icon()));

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

pub fn scan_mods(instances_dir: &Path, instance_name: &str) -> Vec<ContentEntry> {
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

        let Some((enabled, file_stem)) = super::parse_enabled_stem(&file_name, ".jar") else {
            continue;
        };

        entries.push(scan_one_mod(&path, &file_stem, enabled));
    }

    entries.sort_by_cached_key(|e| e.name.to_lowercase());
    entries
}

// tries each loader's metadata file to extract name, description, and icon.
// checks fabric.mod.json, quilt.mod.json, META-INF/mods.toml (forge),
// META-INF/neoforge.mods.toml, and mcmod.info (legacy forge). if none of
// those yield an icon, falls back to common root-level paths.
fn read_mod_metadata(jar_path: &Path) -> (String, String, Option<Vec<u8>>) {
    let file = match std::fs::File::open(jar_path) {
        Ok(file) => file,
        Err(_) => return (String::new(), String::new(), None),
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(_) => return (String::new(), String::new(), None),
    };

    // try each loader's metadata in order. if we get metadata but the
    // declared icon path is missing, fall back to common root-level icons.
    type MetaReader = fn(&mut zip::ZipArchive<std::fs::File>) -> Option<(String, String, String)>;
    let readers: [MetaReader; 4] = [
        read_fabric_meta,
        read_quilt_meta,
        read_forge_toml_meta,
        read_mcmod_info,
    ];

    for reader in &readers {
        if let Some((name, description, icon_path)) = reader(&mut archive) {
            let icon_path = icon_path.trim_start_matches('/');
            let icon = if icon_path.is_empty() {
                None
            } else {
                read_zip_bytes(&mut archive, icon_path)
            }
            .or_else(|| try_fallback_icons(&mut archive));
            return (name, description, icon);
        }
    }

    // no recognized metadata at all, try common icon paths
    let icon_bytes = try_fallback_icons(&mut archive);
    (String::new(), String::new(), icon_bytes)
}

fn try_fallback_icons(archive: &mut zip::ZipArchive<std::fs::File>) -> Option<Vec<u8>> {
    for path in ["logo.png", "icon.png", "pack.png"] {
        if let Some(bytes) = read_zip_bytes(archive, path) {
            return Some(bytes);
        }
    }
    None
}

fn read_fabric_meta(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Option<(String, String, String)> {
    let mut entry = archive.by_name("fabric.mod.json").ok()?;
    let mut raw = String::new();
    entry.read_to_string(&mut raw).ok()?;
    let sanitized = sanitize_json_strings(&raw);
    let data: FabricModJson = serde_json::from_str(&sanitized).ok()?;
    let icon = data.icon_path();
    Some((data.name, data.description, icon))
}

fn read_quilt_meta(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Option<(String, String, String)> {
    let mut entry = archive.by_name("quilt.mod.json").ok()?;
    let mut raw = String::new();
    entry.read_to_string(&mut raw).ok()?;
    let sanitized = sanitize_json_strings(&raw);
    let data: QuiltModJson = serde_json::from_str(&sanitized).ok()?;
    let meta = data.quilt_loader.metadata;
    let icon = meta.icon_path();
    Some((meta.name, meta.description, icon))
}

// forge (META-INF/mods.toml) and neoforge (META-INF/neoforge.mods.toml)
// share the same format. we only need the top-level logoFile and the first
// [[mods]] entry for name/description.
// some jars (e.g. dependency-only libs) have a mods.toml with logoFile
// but no [[mods]] section. we still want the icon in that case.
fn read_forge_toml_meta(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Option<(String, String, String)> {
    let raw = read_zip_string(archive, "META-INF/neoforge.mods.toml")
        .or_else(|| read_zip_string(archive, "META-INF/mods.toml"))?;
    let table: toml::Table = raw.parse().ok()?;
    let logo = table
        .get("logoFile")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let (name, description) = table
        .get("mods")
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_table())
        .map(|first| {
            let n = first
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let d = first
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_owned();
            (n, d)
        })
        .unwrap_or_default();
    Some((name, description, logo))
}

// legacy forge mcmod.info is either a bare json array of mod entries
// or an object with a "modList" key wrapping the array
fn read_mcmod_info(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Option<(String, String, String)> {
    let mut entry = archive.by_name("mcmod.info").ok()?;
    let mut raw = String::new();
    entry.read_to_string(&mut raw).ok()?;
    let sanitized = sanitize_json_strings(&raw);
    let parsed: serde_json::Value = serde_json::from_str(&sanitized).ok()?;
    let first = match &parsed {
        serde_json::Value::Array(arr) => arr.first()?,
        serde_json::Value::Object(obj) => obj.get("modList")?.as_array()?.first()?,
        _ => return None,
    };
    let name = first
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let description = first
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let logo = first
        .get("logoFile")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    Some((name, description, logo))
}

fn read_zip_string(
    archive: &mut zip::ZipArchive<std::fs::File>,
    path: &str,
) -> Option<String> {
    let mut entry = archive.by_name(path).ok()?;
    let mut s = String::new();
    entry.read_to_string(&mut s).ok()?;
    Some(s)
}

// some mod authors put raw newlines/tabs inside json string values which is
// technically invalid json. walks through character by character, tracking
// whether it's inside a string, and escapes the offending characters.
fn sanitize_json_strings(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escape_next = false;

    for ch in input.chars() {
        if escape_next {
            result.push(ch);
            escape_next = false;
            continue;
        }
        if ch == '\\' && in_string {
            result.push(ch);
            escape_next = true;
            continue;
        }
        if ch == '"' {
            in_string = !in_string;
            result.push(ch);
            continue;
        }
        if in_string && ch == '\n' {
            result.push_str("\\n");
        } else if in_string && ch == '\r' {
            result.push_str("\\r");
        } else if in_string && ch == '\t' {
            result.push_str("\\t");
        } else {
            result.push(ch);
        }
    }
    result
}

fn read_zip_bytes(archive: &mut zip::ZipArchive<std::fs::File>, path: &str) -> Option<Vec<u8>> {
    let mut entry = archive.by_name(path).ok()?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

// downscales an icon to terminal resolution. resizes to height*2 because each
// terminal cell renders two vertical pixels using half-block characters (▀)
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
            cols.push(IconCell {
                bg_r: br,
                bg_g: bg,
                bg_b: bb,
                fg_r: tr,
                fg_g: tg,
                fg_b: tb,
            });
        }
        rows.push(cols);
    }

    Some(rows)
}

// 6x3 fallback icon showing a "?" pattern for mods without icons.
pub(super) fn fallback_icon() -> Vec<Vec<IconCell>> {
    let b = IconCell {
        bg_r: 50,
        bg_g: 50,
        bg_b: 50,
        fg_r: 50,
        fg_g: 50,
        fg_b: 50,
    };
    let tb = IconCell {
        bg_r: 50,
        bg_g: 50,
        bg_b: 50,
        fg_r: 130,
        fg_g: 130,
        fg_b: 130,
    };
    let bt = IconCell {
        bg_r: 130,
        bg_g: 130,
        bg_b: 130,
        fg_r: 50,
        fg_g: 50,
        fg_b: 50,
    };
    vec![
        vec![b, tb, tb, tb, tb, b],
        vec![b, b, b, bt, bt, b],
        vec![b, b, bt, bt, b, b],
    ]
}

// 12x6 fallback icon showing a "?" pattern for worlds without icons.
pub(super) fn fallback_icon_large() -> Vec<Vec<IconCell>> {
    let b = IconCell {
        bg_r: 50,
        bg_g: 50,
        bg_b: 50,
        fg_r: 50,
        fg_g: 50,
        fg_b: 50,
    };
    let tb = IconCell {
        bg_r: 50,
        bg_g: 50,
        bg_b: 50,
        fg_r: 130,
        fg_g: 130,
        fg_b: 130,
    };
    let bt = IconCell {
        bg_r: 130,
        bg_g: 130,
        bg_b: 130,
        fg_r: 50,
        fg_g: 50,
        fg_b: 50,
    };
    vec![
        vec![b, b, tb, tb, tb, tb, tb, tb, tb, tb, b, b],
        vec![b, b, tb, tb, tb, tb, tb, tb, tb, tb, b, b],
        vec![b, b, b, b, b, b, bt, bt, bt, bt, b, b],
        vec![b, b, b, b, b, b, bt, bt, bt, bt, b, b],
        vec![b, b, b, b, bt, bt, bt, bt, b, b, b, b],
        vec![b, b, b, b, bt, bt, bt, bt, b, b, b, b],
    ]
}

// enable/disable by renaming the file with/without ".disabled" suffix.
// the minecraft way, apparently.
pub fn toggle_entry(entry: &ContentEntry) -> Result<(), std::io::Error> {
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
    fn toggle_entry_enable() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_mods_dir(tmp.path(), "inst");
        let disabled_path = dir.join("mymod.jar.disabled");
        std::fs::write(&disabled_path, b"PK\x03\x04").unwrap();

        let entry = ContentEntry {
            file_stem: "mymod".to_string(),
            name: "mymod".to_string(),
            description: String::new(),
            enabled: false,
            icon_bytes: None,
            path: disabled_path.clone(),
            icon_lines: None,
        };

        toggle_entry(&entry).unwrap();
        assert!(!disabled_path.exists());
        assert!(dir.join("mymod.jar").exists());
    }

    #[test]
    fn toggle_entry_disable() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_mods_dir(tmp.path(), "inst");
        let enabled_path = dir.join("mymod.jar");
        std::fs::write(&enabled_path, b"PK\x03\x04").unwrap();

        let entry = ContentEntry {
            file_stem: "mymod".to_string(),
            name: "mymod".to_string(),
            description: String::new(),
            enabled: true,
            icon_bytes: None,
            path: enabled_path.clone(),
            icon_lines: None,
        };

        toggle_entry(&entry).unwrap();
        assert!(!enabled_path.exists());
        assert!(dir.join("mymod.jar.disabled").exists());
    }
}
