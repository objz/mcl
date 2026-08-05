// world save scanning. worlds are always directories (never zips) and store
// their icon as icon.png. also computes an approximate size from top-level
// files + region data so the user gets some sense of how chonky their world is.

use std::{fs::File, path::Path};

use flate2::read::GzDecoder;
use serde::Deserialize;

use super::entry::{ContentEntry, WorldDetails, WorldGameMode};
use super::{fallback_icon_large, make_icon_pixels};

pub fn scan_one_world(path: &Path, file_stem: &str, enabled: bool) -> ContentEntry {
    let icon_bytes = std::fs::read(path.join("icon.png")).ok();
    let icon_lines = icon_bytes
        .as_ref()
        .and_then(|bytes| make_icon_pixels(bytes, 12, 6))
        .or_else(|| Some(fallback_icon_large()));

    let metadata = read_world_metadata(path);
    let last_played = world_last_played(path, metadata.as_ref());
    let size = dir_size_approx(path);
    let world_details = WorldDetails {
        game_mode: metadata.as_ref().and_then(WorldMetadata::game_mode),
        last_played,
        minecraft_version: metadata
            .as_ref()
            .and_then(|metadata| metadata.version.as_ref())
            .and_then(|version| version.name.as_deref())
            .filter(|version| !version.trim().is_empty())
            .map(str::to_owned),
        size: (size > 0).then(|| format_size(size)),
        datapacks: datapack_names(path),
    };

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
        world_details: Some(world_details),
        title_suffix: None,
        footer_label: None,
        footer_change: None,
        description: String::new(),
        enabled,
        icon_bytes,
        provider_icon: false,
        provider_description: false,
        path: path.to_path_buf(),
        icon_lines,
    }
}

pub(crate) fn datapack_names(world: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(world.join("datapacks")) else {
        return Vec::new();
    };
    let mut names = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            if path.is_dir() {
                Some(name.trim_end_matches(".disabled").to_owned())
            } else {
                name.strip_suffix(".zip")
                    .or_else(|| name.strip_suffix(".zip.disabled"))
                    .map(str::to_owned)
            }
        })
        .collect::<Vec<_>>();
    names.sort_by_key(|name| name.to_lowercase());
    names
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
    #[serde(rename = "LastPlayed")]
    last_played: Option<i64>,
    #[serde(rename = "Version")]
    version: Option<WorldVersion>,
}

#[derive(Debug, Deserialize)]
struct WorldVersion {
    #[serde(rename = "Name")]
    name: Option<String>,
}

impl WorldMetadata {
    fn game_mode(&self) -> Option<WorldGameMode> {
        if self.hardcore.is_some_and(|hardcore| hardcore != 0) {
            return Some(WorldGameMode::Hardcore);
        }
        Some(match self.game_type? {
            0 => WorldGameMode::Survival,
            1 => WorldGameMode::Creative,
            2 => WorldGameMode::Adventure,
            3 => WorldGameMode::Spectator,
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

fn world_last_played(
    world_dir: &Path,
    metadata: Option<&WorldMetadata>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    let level_dat = world_dir.join("level.dat");
    metadata
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
        })
        .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
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
