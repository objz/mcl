// mod scanning and loader-specific metadata extraction.
// jar files are just zips, so it cracks them open looking for loader-specific
// metadata (fabric.mod.json, quilt.mod.json, mods.toml, mcmod.info) to get
// names, descriptions, and icons. if none of those work, falls back to common
// root-level icon paths (logo.png, icon.png, pack.png) or just the filename.

use std::io::Read;
use std::path::Path;

use serde::Deserialize;

use super::entry::ContentEntry;
use super::icons::{fallback_icon, make_icon_pixels};

#[derive(Deserialize, Default)]
struct FabricModJson {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    version: String,
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
    version: String,
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
    let (name, description, version, icon_bytes) = read_mod_metadata(path);
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
        source_slug: None,
        installed_path: None,
        provider_project: None,
        world_details: None,
        title_suffix: None,
        footer_label: display_version(version),
        footer_change: None,
        description,
        enabled,
        icon_bytes,
        provider_icon: false,
        provider_description: false,
        path: path.to_path_buf(),
        icon_lines,
    }
}

pub fn scan_mods(instances_dir: &Path, instance_name: &str) -> Vec<ContentEntry> {
    let mods_dir = instances_dir
        .join(instance_name)
        .join(crate::storage::MINECRAFT_DIR_NAME)
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
fn read_mod_metadata(jar_path: &Path) -> (String, String, String, Option<Vec<u8>>) {
    let file = match std::fs::File::open(jar_path) {
        Ok(file) => file,
        Err(e) => {
            tracing::trace!("Failed to open mod JAR {}: {}", jar_path.display(), e);
            return (String::new(), String::new(), String::new(), None);
        }
    };

    let mut archive = match zip::ZipArchive::new(file) {
        Ok(archive) => archive,
        Err(e) => {
            tracing::trace!(
                "Failed to read mod JAR as ZIP {}: {}",
                jar_path.display(),
                e
            );
            return (String::new(), String::new(), String::new(), None);
        }
    };

    // try each loader's metadata in order. if we get metadata but the
    // declared icon path is missing, fall back to common root-level icons.
    type MetaReader =
        fn(&mut zip::ZipArchive<std::fs::File>) -> Option<(String, String, String, String)>;
    let readers: [MetaReader; 4] = [
        read_fabric_meta,
        read_quilt_meta,
        read_forge_toml_meta,
        read_mcmod_info,
    ];

    for reader in &readers {
        if let Some((name, description, version, icon_path)) = reader(&mut archive) {
            let icon_path = icon_path.trim_start_matches('/');
            let icon = if icon_path.is_empty() {
                None
            } else {
                read_zip_bytes(&mut archive, icon_path)
            }
            .or_else(|| try_fallback_icons(&mut archive));
            tracing::trace!(
                "Read mod metadata from {}: name='{}' icon_declared={}",
                jar_path.display(),
                name,
                !icon_path.is_empty()
            );
            return (name, description, version, icon);
        }
    }

    // no recognized metadata at all, try common icon paths
    let icon_bytes = try_fallback_icons(&mut archive);
    tracing::trace!(
        "No recognized mod metadata in {}; fallback_icon={}",
        jar_path.display(),
        icon_bytes.is_some()
    );
    (String::new(), String::new(), String::new(), icon_bytes)
}

fn display_version(version: String) -> Option<String> {
    let version = version.trim();
    (!version.is_empty() && !version.contains("${")).then(|| version.to_owned())
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
) -> Option<(String, String, String, String)> {
    let mut entry = archive.by_name("fabric.mod.json").ok()?;
    let mut raw = String::new();
    entry.read_to_string(&mut raw).ok()?;
    let sanitized = sanitize_json_strings(&raw);
    let data: FabricModJson = serde_json::from_str(&sanitized).ok()?;
    let icon = data.icon_path();
    Some((data.name, data.description, data.version, icon))
}

fn read_quilt_meta(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Option<(String, String, String, String)> {
    let mut entry = archive.by_name("quilt.mod.json").ok()?;
    let mut raw = String::new();
    entry.read_to_string(&mut raw).ok()?;
    let sanitized = sanitize_json_strings(&raw);
    let data: QuiltModJson = serde_json::from_str(&sanitized).ok()?;
    let version = data.quilt_loader.version;
    let meta = data.quilt_loader.metadata;
    let icon = meta.icon_path();
    Some((meta.name, meta.description, version, icon))
}

// forge (META-INF/mods.toml) and neoforge (META-INF/neoforge.mods.toml)
// share the same format. we only need the top-level logoFile and the first
// [[mods]] entry for name/description.
// some jars (e.g. dependency-only libs) have a mods.toml with logoFile
// but no [[mods]] section. we still want the icon in that case.
fn read_forge_toml_meta(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Option<(String, String, String, String)> {
    let raw = read_zip_string(archive, "META-INF/neoforge.mods.toml")
        .or_else(|| read_zip_string(archive, "META-INF/mods.toml"))?;
    let table: toml::Table = raw.parse().ok()?;
    let logo = table
        .get("logoFile")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let (name, description, version) = table
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
            let version = first
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            (n, d, version)
        })
        .unwrap_or_default();
    Some((name, description, version, logo))
}

// legacy forge mcmod.info is either a bare json array of mod entries
// or an object with a "modList" key wrapping the array
fn read_mcmod_info(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Option<(String, String, String, String)> {
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
    let version = first
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let logo = first
        .get("logoFile")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    Some((name, description, version, logo))
}

fn read_zip_string(archive: &mut zip::ZipArchive<std::fs::File>, path: &str) -> Option<String> {
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

#[cfg(test)]
#[path = "../tests/content/mods.rs"]
mod tests;
