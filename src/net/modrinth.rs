// modrinth modpack support: fetches project metadata, downloads .mrpack files,
// and extracts loader info from pack manifests.

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

use crate::instance::models::ModLoader;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryKind {
    Mod,
    ResourcePack,
    Shader,
}

impl DiscoveryKind {
    fn project_type(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::ResourcePack => "resourcepack",
            Self::Shader => "shader",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryProject {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub downloads: u64,
    pub icon_url: Option<String>,
    pub icon_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryResults {
    pub projects: Vec<DiscoveryProject>,
    pub total_hits: usize,
}

pub async fn search_discovery(
    client: &crate::net::HttpClient,
    kind: DiscoveryKind,
    query: &str,
    game_version: &str,
    loader: ModLoader,
    offset: usize,
    limit: usize,
) -> Result<DiscoveryResults, crate::net::NetError> {
    let facets = discovery_facets(kind, game_version, loader);
    let mut configuration = modrinth_api::apis::configuration::Configuration::new();
    configuration.client = client.inner().clone();
    configuration.user_agent = Some(format!(
        "rmcl/{} (Minecraft Launcher)",
        env!("CARGO_PKG_VERSION")
    ));
    let query = (!query.trim().is_empty()).then_some(query.trim());
    let index = if query.is_some() {
        "relevance"
    } else {
        "downloads"
    };
    let results = modrinth_api::apis::projects_api::search_projects(
        &configuration,
        query,
        Some(&facets),
        Some(index),
        Some(offset.min(i32::MAX as usize) as i32),
        Some(limit.min(i32::MAX as usize) as i32),
    )
    .await
    .map_err(|e| crate::net::NetError::Parse(format!("Modrinth search failed: {e}")))?;

    Ok(DiscoveryResults {
        total_hits: results.total_hits.max(0) as usize,
        projects: results
            .hits
            .into_iter()
            .map(|project| DiscoveryProject {
                id: project.project_id,
                slug: project.slug,
                title: project.title,
                description: project.description,
                downloads: project.downloads.max(0) as u64,
                icon_url: project.icon_url.flatten(),
                icon_bytes: None,
            })
            .collect(),
    })
}

fn discovery_facets(kind: DiscoveryKind, game_version: &str, loader: ModLoader) -> String {
    let mut facets = vec![vec![format!("project_type:{}", kind.project_type())]];
    if !game_version.is_empty() {
        facets.push(vec![format!("versions:{game_version}")]);
    }
    if kind == DiscoveryKind::Mod
        && let Some(loader) = loader_facet(loader)
    {
        facets.push(vec![format!("categories:{loader}")]);
    }
    serde_json::to_string(&facets).unwrap_or_else(|_| "[]".to_string())
}

fn loader_facet(loader: ModLoader) -> Option<&'static str> {
    match loader {
        ModLoader::Vanilla => None,
        ModLoader::Fabric => Some("fabric"),
        ModLoader::Forge => Some("forge"),
        ModLoader::NeoForge => Some("neoforge"),
        ModLoader::Quilt => Some("quilt"),
    }
}

// mrpack dependencies use keys like "fabric-loader", "forge", etc.
// checks in priority order and returns the first match.
pub fn loader_from_dependencies(
    deps: &HashMap<String, String>,
) -> (Option<ModLoader>, Option<String>) {
    let loaders = [
        ("fabric-loader", ModLoader::Fabric),
        ("forge", ModLoader::Forge),
        ("neoforge", ModLoader::NeoForge),
        ("quilt-loader", ModLoader::Quilt),
    ];
    for (key, loader) in &loaders {
        if let Some(version) = deps.get(*key) {
            tracing::trace!(
                "Resolved Modrinth loader dependency {}={} as {}",
                key,
                version,
                loader
            );
            return (Some(*loader), Some(version.clone()));
        }
    }
    tracing::trace!("No Modrinth loader dependency found; treating pack as vanilla");
    (None, None)
}

pub fn game_version_from_dependencies(deps: &HashMap<String, String>) -> Option<String> {
    deps.get("minecraft").cloned()
}

const API_BASE: &str = "https://api.modrinth.com/v2";

// hand-rolled percent encoding because pulling in a crate for RFC 3986
// unreserved chars felt like overkill
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
    tracing::debug!("Fetching Modrinth project '{}'", slug_or_id);
    let project: ProjectInfo = client.get_json(&url).await?;
    tracing::debug!(
        "Fetched Modrinth project '{}' ({})",
        project.slug,
        project.id
    );
    Ok(project)
}

pub async fn fetch_versions(
    client: &crate::net::HttpClient,
    slug_or_id: &str,
) -> Result<Vec<VersionInfo>, crate::net::NetError> {
    let url = format!(
        "{}/project/{}/version?loaders=[\"fabric\",\"forge\",\"neoforge\",\"quilt\"]",
        API_BASE,
        url_encode(slug_or_id)
    );
    tracing::debug!("Fetching Modrinth versions for project '{}'", slug_or_id);
    let versions: Vec<VersionInfo> = client.get_json(&url).await?;
    tracing::debug!(
        "Fetched {} Modrinth version(s) for project '{}'",
        versions.len(),
        slug_or_id
    );
    Ok(versions)
}

pub async fn fetch_version(
    client: &crate::net::HttpClient,
    version_id: &str,
) -> Result<VersionInfo, crate::net::NetError> {
    let url = format!("{}/version/{}", API_BASE, url_encode(version_id));
    tracing::debug!("Fetching Modrinth version '{}'", version_id);
    let version: VersionInfo = client.get_json(&url).await?;
    tracing::debug!(
        "Fetched Modrinth version '{}' ({}) with {} file(s)",
        version.name,
        version.id,
        version.files.len()
    );
    Ok(version)
}

// grabs the primary file from a version, falling back to the first file
// if none is marked primary (some projects are sloppy about that)
pub async fn download_mrpack(
    client: &crate::net::HttpClient,
    version: &VersionInfo,
    dest: &std::path::Path,
) -> Result<std::path::PathBuf, crate::net::NetError> {
    let file = version
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| {
            tracing::warn!(
                "Modrinth version '{}' has no primary file; using first file",
                version.id
            );
            version.files.first()
        })
        .ok_or_else(|| crate::net::NetError::Parse("No files in version".to_string()))?;

    let mrpack_path = dest.join(&file.filename);
    tracing::info!(
        "Downloading Modrinth pack file '{}' for version '{}' to {}",
        file.filename,
        version.id,
        mrpack_path.display()
    );
    crate::net::download_file(client, &file.url, &mrpack_path, |_, _| {}).await?;
    Ok(mrpack_path)
}

// .mrpack is just a zip with modrinth.index.json at the root
pub fn parse_mrpack(path: &std::path::Path) -> Result<MrpackIndex, String> {
    tracing::debug!("Parsing .mrpack manifest from {}", path.display());
    let file = std::fs::File::open(path).map_err(|e| format!("Cannot open .mrpack: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Invalid ZIP: {e}"))?;
    let entry = archive
        .by_name("modrinth.index.json")
        .map_err(|_| "Missing modrinth.index.json in .mrpack".to_string())?;
    let index: MrpackIndex =
        serde_json::from_reader(entry).map_err(|e| format!("Invalid manifest JSON: {e}"))?;
    tracing::debug!(
        "Parsed .mrpack '{}' version_id={} files={} deps={}",
        index.name,
        index.version_id,
        index.files.len(),
        index.dependencies.len()
    );
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_mod_facets_include_instance_compatibility() {
        let facets = discovery_facets(DiscoveryKind::Mod, "1.21.1", ModLoader::Fabric);
        assert_eq!(
            serde_json::from_str::<Vec<Vec<String>>>(&facets).unwrap(),
            vec![
                vec!["project_type:mod"],
                vec!["versions:1.21.1"],
                vec!["categories:fabric"],
            ]
        );
    }

    #[test]
    fn discovery_resource_pack_facets_do_not_require_loader() {
        let facets = discovery_facets(DiscoveryKind::ResourcePack, "1.20.1", ModLoader::Forge);
        assert_eq!(
            serde_json::from_str::<Vec<Vec<String>>>(&facets).unwrap(),
            vec![vec!["project_type:resourcepack"], vec!["versions:1.20.1"]]
        );
    }

    #[tokio::test]
    #[ignore = "hits live Modrinth API"]
    async fn search_discovery_returns_compatible_mods() {
        let client = crate::net::HttpClient::new();
        let result = search_discovery(
            &client,
            DiscoveryKind::Mod,
            "sodium",
            "1.21.1",
            ModLoader::Fabric,
            0,
            20,
        )
        .await
        .unwrap();

        assert!(!result.projects.is_empty());
        assert!(result.total_hits >= result.projects.len());
        let icon_url = result
            .projects
            .iter()
            .find_map(|project| project.icon_url.as_deref())
            .expect("expected at least one project icon");
        let icon = client.get_bytes(icon_url).await.unwrap();
        assert!(
            image::load_from_memory(&icon).is_ok(),
            "failed to decode {icon_url}; first bytes: {:?}",
            icon.get(..icon.len().min(16))
        );

        let next = search_discovery(
            &client,
            DiscoveryKind::Mod,
            "sodium",
            "1.21.1",
            ModLoader::Fabric,
            20,
            20,
        )
        .await
        .unwrap();
        assert!(!next.projects.is_empty());
        assert!(
            next.projects
                .iter()
                .all(|project| !result.projects.iter().any(|first| first.id == project.id))
        );
    }

    // covers each branch of url_encode: unreserved bytes pass through; the
    // reserved set + spaces + non-ascii bytes get percent-encoded. emoji
    // exercises multi-byte UTF-8 since the encoder operates on bytes, not
    // chars, so each byte of the codepoint encodes separately.
    #[rstest::rstest]
    #[case::ascii_unreserved("abcXYZ0-9_.~", "abcXYZ0-9_.~")]
    #[case::space("hello world", "hello%20world")]
    #[case::reserved("/?&=#", "%2F%3F%26%3D%23")]
    #[case::utf8_emoji("\u{2603}", "%E2%98%83")]
    #[case::empty("", "")]
    fn url_encode_handles(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(url_encode(input), expected);
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
        assert_eq!(
            game_version_from_dependencies(&deps),
            Some("1.21.4".to_string())
        );
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
