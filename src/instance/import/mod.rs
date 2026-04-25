// modpack importing: parses user input, detects pack format from zip contents,
// builds a summary, and delegates the actual import to format-specific modules.

pub mod mmc;
pub mod mrpack;

use std::path::{Path, PathBuf};

use crate::instance::manager::InstanceManager;
use crate::instance::models::ModLoader;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackFormat {
    Mrpack,
    Mmc,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportInput {
    ProjectSlug(String),
    VersionId { slug: String, version_id: String },
    LocalFile(String),
}

// figures out what the user gave us: a modrinth URL, a local pack file,
// or just a project slug. accepts a pretty wide range of inputs so users
// don't have to think about it.
pub fn parse_import_input(input: &str) -> ImportInput {
    let input = input.trim();

    if input.ends_with(".mrpack")
        || input.ends_with(".zip")
        || input.starts_with('/')
        || input.starts_with("~/")
    {
        return ImportInput::LocalFile(input.to_string());
    }

    if let Some(rest) = input
        .strip_prefix("https://modrinth.com/modpack/")
        .or_else(|| input.strip_prefix("http://modrinth.com/modpack/"))
    {
        let parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
        return match parts.as_slice() {
            [slug, "version", version_id, ..] => ImportInput::VersionId {
                slug: slug.to_string(),
                version_id: version_id.to_string(),
            },
            [slug, ..] => ImportInput::ProjectSlug(slug.to_string()),
            [] => ImportInput::ProjectSlug(String::new()),
        };
    }

    ImportInput::ProjectSlug(input.to_string())
}

#[derive(Debug, Clone)]
pub struct ImportSummary {
    pub name: String,
    pub pack_version: String,
    pub game_version: String,
    pub loader: ModLoader,
    pub loader_version: Option<String>,
    pub mod_count: usize,
    pub override_count: usize,
    pub format: PackFormat,
    pub archive_path: PathBuf,
}

// peeks inside a zip to figure out what format it is.
// checks for modrinth.index.json first, then mmc-pack.json.
pub fn detect_format(path: &Path) -> Result<PackFormat, String> {
    let file =
        std::fs::File::open(path).map_err(|e| format!("Cannot open '{}': {e}", path.display()))?;
    let archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Invalid ZIP '{}': {e}", path.display()))?;

    if archive.file_names().any(|n| n == "modrinth.index.json") {
        return Ok(PackFormat::Mrpack);
    }

    // mmc-pack.json can be at root or one directory deep
    if archive
        .file_names()
        .any(|n| n == "mmc-pack.json" || n.ends_with("/mmc-pack.json"))
    {
        return Ok(PackFormat::Mmc);
    }

    Err("Unknown pack format: no modrinth.index.json or mmc-pack.json found".to_string())
}

pub fn build_summary(path: &Path) -> Result<ImportSummary, String> {
    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }
    let format = detect_format(path)?;
    match format {
        PackFormat::Mrpack => mrpack::build_summary(path),
        PackFormat::Mmc => mmc::build_summary(path),
    }
}

pub fn unique_instance_name(base: &str, instances_dir: &Path) -> String {
    let candidate = base.to_string();
    if !instances_dir
        .join(&candidate)
        .join("instance.json")
        .exists()
    {
        return candidate;
    }
    for n in 2..100 {
        let candidate = format!("{base} ({n})");
        if !instances_dir
            .join(&candidate)
            .join("instance.json")
            .exists()
        {
            return candidate;
        }
    }
    format!("{base} (import)")
}

pub async fn execute_import(
    summary: &ImportSummary,
    manager: &InstanceManager,
) -> Result<crate::instance::InstanceConfig, Box<dyn std::error::Error + Send + Sync>> {
    match summary.format {
        PackFormat::Mrpack => mrpack::execute_import(summary, manager).await,
        PackFormat::Mmc => mmc::execute_import(summary, manager).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_name_no_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let name = unique_instance_name("TestPack", tmp.path());
        assert_eq!(name, "TestPack");
    }

    #[test]
    fn unique_name_with_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("TestPack");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("instance.json"), "{}").unwrap();
        let name = unique_instance_name("TestPack", tmp.path());
        assert_eq!(name, "TestPack (2)");
    }

    #[test]
    fn unique_name_multiple_collisions() {
        let tmp = tempfile::tempdir().unwrap();
        for suffix in ["", " (2)", " (3)"] {
            let dir = tmp.path().join(format!("TestPack{suffix}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("instance.json"), "{}").unwrap();
        }
        let name = unique_instance_name("TestPack", tmp.path());
        assert_eq!(name, "TestPack (4)");
    }

    #[test]
    fn parse_project_url() {
        assert_eq!(
            parse_import_input("https://modrinth.com/modpack/fabulously-optimized"),
            ImportInput::ProjectSlug("fabulously-optimized".to_string())
        );
    }

    #[test]
    fn parse_version_url() {
        assert_eq!(
            parse_import_input("https://modrinth.com/modpack/fabulously-optimized/version/abc123"),
            ImportInput::VersionId {
                slug: "fabulously-optimized".to_string(),
                version_id: "abc123".to_string(),
            }
        );
    }

    #[test]
    fn parse_local_mrpack() {
        assert_eq!(
            parse_import_input("/home/user/pack.mrpack"),
            ImportInput::LocalFile("/home/user/pack.mrpack".to_string())
        );
    }

    #[test]
    fn parse_local_zip() {
        assert_eq!(
            parse_import_input("GT_New_Horizons.zip"),
            ImportInput::LocalFile("GT_New_Horizons.zip".to_string())
        );
    }

    #[test]
    fn parse_tilde_path() {
        assert_eq!(
            parse_import_input("~/Downloads/pack.mrpack"),
            ImportInput::LocalFile("~/Downloads/pack.mrpack".to_string())
        );
    }

    #[test]
    fn parse_bare_slug() {
        assert_eq!(
            parse_import_input("fabulously-optimized"),
            ImportInput::ProjectSlug("fabulously-optimized".to_string())
        );
    }

    #[test]
    fn parse_input_trims_whitespace() {
        assert_eq!(
            parse_import_input("  fabulously-optimized  "),
            ImportInput::ProjectSlug("fabulously-optimized".to_string())
        );
    }
}
