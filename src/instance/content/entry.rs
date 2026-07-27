use std::path::PathBuf;

use super::icons::IconCell;

#[derive(Debug, Clone)]
pub struct ContentEntry {
    pub file_stem: String,
    pub name: String,
    pub source_slug: Option<String>,
    pub installed_path: Option<PathBuf>,
    pub provider_project: Option<super::manifest::ProviderProject>,
    pub title_suffix: Option<String>,
    pub footer_label: Option<String>,
    pub description: String,
    pub enabled: bool,
    pub icon_bytes: Option<Vec<u8>>,
    pub provider_icon: bool,
    pub provider_description: bool,
    pub path: PathBuf,
    pub icon_lines: Option<Vec<Vec<IconCell>>>,
}

// enable/disable by renaming the file with/without ".disabled" suffix.
pub fn toggle_entry(entry: &ContentEntry) -> Result<(), std::io::Error> {
    toggle_entry_path(entry).map(drop)
}

pub(crate) fn toggle_entry_path(entry: &ContentEntry) -> Result<Option<PathBuf>, std::io::Error> {
    let Some(file_name) = entry.path.file_name().and_then(|name| name.to_str()) else {
        return Ok(None);
    };
    let new_name = if entry.enabled {
        format!("{file_name}.disabled")
    } else {
        file_name.trim_end_matches(".disabled").to_owned()
    };
    let mut new_path = entry.path.clone();
    new_path.set_file_name(new_name);
    std::fs::rename(&entry.path, &new_path)?;
    Ok(Some(new_path))
}
