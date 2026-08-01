// world save scanning. worlds are always directories (never zips) and store
// their icon as icon.png. also computes an approximate size from top-level
// files + region data so the user gets some sense of how chonky their world is.

use std::{fs::File, path::Path};

use flate2::read::GzDecoder;
use serde::Deserialize;

use super::entry::ContentEntry;
use super::{fallback_icon_large, make_icon_pixels};

pub fn scan_one_world(path: &Path, file_stem: &str, enabled: bool) -> ContentEntry {
    let icon_bytes = std::fs::read(path.join("icon.png")).ok();
    let icon_lines = icon_bytes
        .as_ref()
        .and_then(|bytes| make_icon_pixels(bytes, 12, 6))
        .or_else(|| Some(fallback_icon_large()));

    let metadata = read_world_metadata(path);
    let description = world_description(path, metadata.as_ref());

    ContentEntry {
        name: metadata
            .as_ref()
            .and_then(|metadata| metadata.level_name.as_deref())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(file_stem)
            .to_owned(),
        file_stem: file_stem.to_owned(),
        source_slug: None,
        installed_path: None,
        provider_project: None,
        title_suffix: metadata.as_ref().and_then(WorldMetadata::game_mode),
        footer_label: None,
        description,
        enabled,
        icon_bytes,
        provider_icon: false,
        provider_description: false,
        path: path.to_path_buf(),
        icon_lines,
    }
}

pub fn scan_worlds(instances_dir: &Path, instance_name: &str) -> Vec<ContentEntry> {
    let saves_dir = instances_dir
        .join(instance_name)
        .join(crate::storage::MINECRAFT_DIR_NAME)
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

#[derive(Debug, Deserialize)]
struct LevelDat {
    #[serde(rename = "Data")]
    data: WorldMetadata,
}

#[derive(Debug, Deserialize)]
struct WorldMetadata {
    #[serde(rename = "LevelName")]
    level_name: Option<String>,
    #[serde(rename = "GameType")]
    game_type: Option<i32>,
    #[serde(rename = "hardcore")]
    hardcore: Option<i8>,
    #[serde(rename = "Difficulty")]
    difficulty: Option<i8>,
    #[serde(rename = "allowCommands")]
    allow_commands: Option<i8>,
    #[serde(rename = "LastPlayed")]
    last_played: Option<i64>,
    #[serde(rename = "Version")]
    version: Option<WorldVersion>,
    #[serde(rename = "DataVersion")]
    data_version: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct WorldVersion {
    #[serde(rename = "Name")]
    name: Option<String>,
}

impl WorldMetadata {
    fn game_mode(&self) -> Option<String> {
        if self.hardcore.is_some_and(|hardcore| hardcore != 0) {
            return Some("Hardcore".to_owned());
        }
        Some(
            match self.game_type? {
                0 => "Survival",
                1 => "Creative",
                2 => "Adventure",
                3 => "Spectator",
                _ => return None,
            }
            .to_owned(),
        )
    }

    fn difficulty(&self) -> Option<&'static str> {
        Some(match self.difficulty? {
            0 => "Peaceful",
            1 => "Easy",
            2 => "Normal",
            3 => "Hard",
            _ => return None,
        })
    }
}

fn read_world_metadata(world_dir: &Path) -> Option<WorldMetadata> {
    let path = world_dir.join("level.dat");
    let file = File::open(&path).ok()?;
    match fastnbt::from_reader::<_, LevelDat>(GzDecoder::new(file)) {
        Ok(level) => Some(level.data),
        Err(error) => {
            tracing::debug!(
                "Could not read world metadata from {}: {error}",
                path.display()
            );
            None
        }
    }
}

fn world_description(world_dir: &Path, metadata: Option<&WorldMetadata>) -> String {
    let level_dat = world_dir.join("level.dat");

    let created = world_dir
        .metadata()
        .ok()
        .and_then(|m| m.created().ok().or_else(|| m.modified().ok()))
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    let modified = metadata
        .and_then(|metadata| metadata.last_played)
        .filter(|millis| *millis > 0)
        .map(|millis| millis / 1000)
        .or_else(|| {
            level_dat
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
        });

    let dir_size = dir_size_approx(world_dir);

    let mut lines = Vec::new();

    if let Some(secs) = modified
        && let Some(dt) = chrono::DateTime::from_timestamp(secs, 0)
    {
        lines.push(format!("Last played:  {}", dt.format("%Y-%m-%d %H:%M")));
    }

    if let Some(metadata) = metadata {
        let mut settings = Vec::new();
        if let Some(difficulty) = metadata.difficulty() {
            settings.push(format!("Difficulty: {difficulty}"));
        }
        if let Some(allow_commands) = metadata.allow_commands {
            settings.push(format!(
                "Cheats: {}",
                if allow_commands == 0 { "Off" } else { "On" }
            ));
        }
        if !settings.is_empty() {
            lines.push(settings.join("  •  "));
        }

        if let Some(version) = metadata
            .version
            .as_ref()
            .and_then(|version| version.name.as_deref())
            .filter(|version| !version.trim().is_empty())
        {
            lines.push(format!("Minecraft:    {version}"));
        } else if let Some(data_version) = metadata.data_version {
            lines.push(format!("Data version: {data_version}"));
        }
    }

    if let Some(secs) = created
        && let Some(dt) = chrono::DateTime::from_timestamp(secs as i64, 0)
    {
        lines.push(format!("Created:      {}", dt.format("%Y-%m-%d %H:%M")));
    }

    if dir_size > 0 {
        lines.push(format!("Approx. size: {}", format_size(dir_size)));
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
#[path = "../tests/content/worlds.rs"]
mod tests;
