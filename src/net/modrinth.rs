use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct ProjectInfo {
    pub id: String,
    pub slug: String,
    pub title: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionInfo {
    pub id: String,
    pub name: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub files: Vec<VersionFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionFile {
    pub url: String,
    pub filename: String,
    pub size: u64,
    pub primary: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MrpackIndex {
    #[serde(rename = "formatVersion")]
    pub format_version: u32,
    pub game: String,
    #[serde(rename = "versionId")]
    pub version_id: String,
    pub name: String,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
    #[serde(default)]
    pub files: Vec<MrpackFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MrpackFile {
    pub path: String,
    pub downloads: Vec<String>,
    #[serde(rename = "fileSize")]
    pub file_size: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModrinthInput {
    ProjectSlug(String),
    VersionId { slug: String, version_id: String },
    LocalFile(String),
}

pub fn parse_input(input: &str) -> ModrinthInput {
    let input = input.trim();

    if input.ends_with(".mrpack")
        || input.starts_with('/')
        || input.starts_with("~/")
    {
        return ModrinthInput::LocalFile(input.to_string());
    }

    if let Some(rest) = input
        .strip_prefix("https://modrinth.com/modpack/")
        .or_else(|| input.strip_prefix("http://modrinth.com/modpack/"))
    {
        let parts: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
        return match parts.as_slice() {
            [slug, "version", version_id, ..] => ModrinthInput::VersionId {
                slug: slug.to_string(),
                version_id: version_id.to_string(),
            },
            [slug, ..] => ModrinthInput::ProjectSlug(slug.to_string()),
            [] => ModrinthInput::ProjectSlug(String::new()),
        };
    }

    ModrinthInput::ProjectSlug(input.to_string())
}

use crate::instance::models::ModLoader;

pub fn loader_from_dependencies(deps: &HashMap<String, String>) -> (Option<ModLoader>, Option<String>) {
    let loaders = [
        ("fabric-loader", ModLoader::Fabric),
        ("forge", ModLoader::Forge),
        ("neoforge", ModLoader::NeoForge),
        ("quilt-loader", ModLoader::Quilt),
    ];
    for (key, loader) in &loaders {
        if let Some(version) = deps.get(*key) {
            return (Some(*loader), Some(version.clone()));
        }
    }
    (None, None)
}

pub fn game_version_from_dependencies(deps: &HashMap<String, String>) -> Option<String> {
    deps.get("minecraft").cloned()
}

const API_BASE: &str = "https://api.modrinth.com/v2";

fn url_encode(s: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                write!(encoded, "%{byte:02X}").unwrap();
            }
        }
    }
    encoded
}

pub async fn fetch_project(
    client: &crate::net::HttpClient,
    slug_or_id: &str,
) -> Result<ProjectInfo, crate::net::NetError> {
    let url = format!("{}/project/{}", API_BASE, url_encode(slug_or_id));
    client.get_json(&url).await
}

pub async fn fetch_versions(
    client: &crate::net::HttpClient,
    slug_or_id: &str,
) -> Result<Vec<VersionInfo>, crate::net::NetError> {
    let url = format!(
        "{}/project/{}/version?loaders=[\"fabric\",\"forge\",\"neoforge\",\"quilt\"]",
        API_BASE, url_encode(slug_or_id)
    );
    client.get_json(&url).await
}

pub async fn fetch_version(
    client: &crate::net::HttpClient,
    version_id: &str,
) -> Result<VersionInfo, crate::net::NetError> {
    let url = format!("{}/version/{}", API_BASE, url_encode(version_id));
    client.get_json(&url).await
}

pub async fn download_mrpack(
    client: &crate::net::HttpClient,
    version: &VersionInfo,
    dest: &std::path::Path,
) -> Result<std::path::PathBuf, crate::net::NetError> {
    let file = version
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| crate::net::NetError::Parse("No files in version".to_string()))?;

    let mrpack_path = dest.join(&file.filename);
    crate::net::download_file(client, &file.url, &mrpack_path, |_, _| {}).await?;
    Ok(mrpack_path)
}

pub fn parse_mrpack(path: &std::path::Path) -> Result<MrpackIndex, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Cannot open .mrpack: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Invalid ZIP: {e}"))?;
    let entry = archive
        .by_name("modrinth.index.json")
        .map_err(|_| "Missing modrinth.index.json in .mrpack".to_string())?;
    serde_json::from_reader(entry).map_err(|e| format!("Invalid manifest JSON: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_project_url() {
        assert_eq!(
            parse_input("https://modrinth.com/modpack/fabulously-optimized"),
            ModrinthInput::ProjectSlug("fabulously-optimized".to_string())
        );
    }

    #[test]
    fn parse_version_url() {
        let result = parse_input("https://modrinth.com/modpack/fabulously-optimized/version/abc123");
        assert_eq!(
            result,
            ModrinthInput::VersionId {
                slug: "fabulously-optimized".to_string(),
                version_id: "abc123".to_string(),
            }
        );
    }

    #[test]
    fn parse_local_mrpack() {
        assert_eq!(
            parse_input("/home/user/pack.mrpack"),
            ModrinthInput::LocalFile("/home/user/pack.mrpack".to_string())
        );
    }

    #[test]
    fn parse_tilde_path() {
        assert_eq!(
            parse_input("~/Downloads/pack.mrpack"),
            ModrinthInput::LocalFile("~/Downloads/pack.mrpack".to_string())
        );
    }

    #[test]
    fn parse_bare_slug() {
        assert_eq!(
            parse_input("fabulously-optimized"),
            ModrinthInput::ProjectSlug("fabulously-optimized".to_string())
        );
    }

    #[test]
    fn parse_input_trims_whitespace() {
        assert_eq!(
            parse_input("  fabulously-optimized  "),
            ModrinthInput::ProjectSlug("fabulously-optimized".to_string())
        );
    }

    #[test]
    fn loader_from_fabric_deps() {
        let mut deps = HashMap::new();
        deps.insert("minecraft".to_string(), "1.21.4".to_string());
        deps.insert("fabric-loader".to_string(), "0.16.10".to_string());
        let (loader, version) = loader_from_dependencies(&deps);
        assert_eq!(loader, Some(ModLoader::Fabric));
        assert_eq!(version, Some("0.16.10".to_string()));
    }

    #[test]
    fn loader_from_forge_deps() {
        let mut deps = HashMap::new();
        deps.insert("minecraft".to_string(), "1.20.1".to_string());
        deps.insert("forge".to_string(), "47.2.0".to_string());
        let (loader, version) = loader_from_dependencies(&deps);
        assert_eq!(loader, Some(ModLoader::Forge));
        assert_eq!(version, Some("47.2.0".to_string()));
    }

    #[test]
    fn loader_from_vanilla_deps() {
        let mut deps = HashMap::new();
        deps.insert("minecraft".to_string(), "1.21.4".to_string());
        let (loader, version) = loader_from_dependencies(&deps);
        assert!(loader.is_none());
        assert!(version.is_none());
    }

    #[test]
    fn game_version_from_deps() {
        let mut deps = HashMap::new();
        deps.insert("minecraft".to_string(), "1.21.4".to_string());
        assert_eq!(game_version_from_dependencies(&deps), Some("1.21.4".to_string()));
    }

    #[test]
    fn parse_mrpack_index_json() {
        let json = r#"{
            "formatVersion": 1,
            "game": "minecraft",
            "versionId": "6.5.0",
            "name": "Fabulously Optimized",
            "dependencies": {
                "minecraft": "1.21.4",
                "fabric-loader": "0.16.10"
            },
            "files": [
                {
                    "path": "mods/fabric-api.jar",
                    "downloads": ["https://cdn.modrinth.com/data/abc/fabric-api.jar"],
                    "fileSize": 12345
                }
            ]
        }"#;
        let index: MrpackIndex = serde_json::from_str(json).unwrap();
        assert_eq!(index.name, "Fabulously Optimized");
        assert_eq!(index.version_id, "6.5.0");
        assert_eq!(index.files.len(), 1);
        assert_eq!(index.files[0].path, "mods/fabric-api.jar");
        assert_eq!(
            game_version_from_dependencies(&index.dependencies),
            Some("1.21.4".to_string())
        );
    }

    #[tokio::test]
    #[ignore = "hits live Modrinth API"]
    async fn test_fetch_project() {
        let client = crate::net::HttpClient::new();
        let project = fetch_project(&client, "fabulously-optimized").await;
        match project {
            Ok(p) => {
                assert_eq!(p.slug, "fabulously-optimized");
                assert!(!p.title.is_empty());
            }
            Err(e) => panic!("fetch_project failed: {e}"),
        }
    }

    #[tokio::test]
    #[ignore = "hits live Modrinth API"]
    async fn test_fetch_versions() {
        let client = crate::net::HttpClient::new();
        let versions = fetch_versions(&client, "fabulously-optimized").await;
        match versions {
            Ok(v) => {
                assert!(!v.is_empty());
                assert!(!v[0].files.is_empty());
            }
            Err(e) => panic!("fetch_versions failed: {e}"),
        }
    }
}
