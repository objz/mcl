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

#[derive(Debug, Deserialize)]
struct DiscoverySearchResponse {
    hits: Vec<DiscoverySearchHit>,
    #[serde(default)]
    total_hits: i64,
}

#[derive(Debug, Deserialize)]
struct DiscoverySearchHit {
    project_id: String,
    slug: String,
    title: String,
    description: String,
    #[serde(default)]
    downloads: i64,
    #[serde(default)]
    icon_url: Option<String>,
}

impl From<DiscoverySearchHit> for DiscoveryProject {
    fn from(project: DiscoverySearchHit) -> Self {
        Self {
            id: project.project_id,
            slug: project.slug,
            title: project.title,
            description: project.description,
            downloads: project.downloads.max(0) as u64,
            icon_url: project.icon_url.filter(|url| !url.trim().is_empty()),
            icon_bytes: None,
        }
    }
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
    let query = query.trim();
    let index = if query.is_empty() {
        "downloads"
    } else {
        "relevance"
    };
    let url = format!(
        "{API_BASE}/search?query={}&facets={}&index={index}&offset={offset}&limit={limit}",
        url_encode(query),
        url_encode(&facets),
    );
    let results: DiscoverySearchResponse = client.get_json(&url).await?;

    Ok(DiscoveryResults {
        total_hits: results.total_hits.max(0) as usize,
        projects: results
            .hits
            .into_iter()
            .map(DiscoveryProject::from)
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

pub async fn fetch_content_versions(
    client: &crate::net::HttpClient,
    project_id: &str,
    kind: DiscoveryKind,
    game_version: &str,
    loader: ModLoader,
) -> Result<Vec<VersionInfo>, crate::net::NetError> {
    let url = content_versions_url(API_BASE, project_id, kind, game_version, loader);
    tracing::debug!(
        "Fetching compatible Modrinth versions for '{}' ({}, {})",
        project_id,
        game_version,
        loader
    );
    client.get_json(&url).await
}

fn content_versions_url(
    api_base: &str,
    project_id: &str,
    kind: DiscoveryKind,
    game_version: &str,
    loader: ModLoader,
) -> String {
    let mut params = vec![
        "include_changelog=false".to_owned(),
        format!(
            "game_versions={}",
            url_encode(&serde_json::to_string(&[game_version]).unwrap_or_default())
        ),
    ];
    if kind == DiscoveryKind::Mod
        && let Some(loader) = loader_facet(loader)
    {
        params.push(format!(
            "loaders={}",
            url_encode(&serde_json::to_string(&[loader]).unwrap_or_default())
        ));
    }
    format!(
        "{api_base}/project/{}/version?{}",
        url_encode(project_id),
        params.join("&")
    )
}

pub fn select_primary_file(version: &VersionInfo) -> Result<&VersionFile, crate::net::NetError> {
    version
        .files
        .iter()
        .find(|file| file.primary)
        .or_else(|| version.files.first())
        .ok_or_else(|| crate::net::NetError::Parse("No files in version".to_owned()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadOutcome {
    Downloaded(std::path::PathBuf),
    SkippedExisting(std::path::PathBuf),
}

pub async fn download_version_file(
    client: &crate::net::HttpClient,
    version: &VersionInfo,
    destination: &std::path::Path,
) -> Result<DownloadOutcome, crate::net::NetError> {
    let file = select_primary_file(version)?;
    let path = destination.join(&file.filename);
    if path.exists() {
        return Ok(DownloadOutcome::SkippedExisting(path));
    }
    crate::net::download_file(client, &file.url, &path, |_, _| {}).await?;
    Ok(DownloadOutcome::Downloaded(path))
}

pub async fn download_version_file_for_update(
    client: &crate::net::HttpClient,
    version: &VersionInfo,
    destination: &std::path::Path,
    installed_path: &std::path::Path,
) -> Result<DownloadOutcome, crate::net::NetError> {
    let file = select_primary_file(version)?;
    let target = destination.join(&file.filename);
    if target != installed_path {
        return download_version_file(client, version, destination).await;
    }

    let temporary = destination.join(format!(".{}.{}.rmcl-download", file.filename, version.id));
    let backup = destination.join(format!(".{}.{}.rmcl-backup", file.filename, version.id));
    if temporary.exists() || backup.exists() {
        return Err(crate::net::NetError::Parse(format!(
            "A previous update of '{}' did not finish cleanly",
            file.filename
        )));
    }

    crate::net::download_file(client, &file.url, &temporary, |_, _| {}).await?;
    replace_installed_file(&temporary, &target, installed_path, &backup).await?;
    Ok(DownloadOutcome::Downloaded(target))
}

async fn replace_installed_file(
    temporary: &std::path::Path,
    target: &std::path::Path,
    installed_path: &std::path::Path,
    backup: &std::path::Path,
) -> Result<(), crate::net::NetError> {
    tokio::fs::rename(installed_path, &backup).await?;
    if let Err(error) = tokio::fs::rename(&temporary, &target).await {
        let _ = tokio::fs::rename(&backup, installed_path).await;
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(error.into());
    }
    if let Err(error) = tokio::fs::remove_file(&backup).await {
        tracing::warn!("Failed to remove Modrinth update backup: {error}");
    }
    Ok(())
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
    let file = select_primary_file(version)?;

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

    fn version_with_files(files: Vec<VersionFile>) -> VersionInfo {
        VersionInfo {
            id: "version-id".to_owned(),
            name: "Version 1".to_owned(),
            version_number: "1.0.0".to_owned(),
            game_versions: vec!["1.21.1".to_owned()],
            loaders: vec!["fabric".to_owned()],
            files,
        }
    }

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

    #[test]
    fn compatible_mod_versions_filter_by_game_and_loader() {
        let url = content_versions_url(
            "https://example.test/v2",
            "fabric-api",
            DiscoveryKind::Mod,
            "1.21.1",
            ModLoader::Fabric,
        );
        assert_eq!(
            url,
            "https://example.test/v2/project/fabric-api/version?include_changelog=false&game_versions=%5B%221.21.1%22%5D&loaders=%5B%22fabric%22%5D"
        );
    }

    #[test]
    fn compatible_resource_pack_versions_do_not_filter_by_loader() {
        let url = content_versions_url(
            "https://example.test/v2",
            "stay-true",
            DiscoveryKind::ResourcePack,
            "1.21.1",
            ModLoader::Fabric,
        );
        assert_eq!(
            url,
            "https://example.test/v2/project/stay-true/version?include_changelog=false&game_versions=%5B%221.21.1%22%5D"
        );
    }

    #[test]
    fn primary_file_selection_falls_back_to_first_file() {
        let version = version_with_files(vec![
            VersionFile {
                url: "https://example.test/first.jar".to_owned(),
                filename: "first.jar".to_owned(),
                size: 1,
                primary: false,
            },
            VersionFile {
                url: "https://example.test/primary.jar".to_owned(),
                filename: "primary.jar".to_owned(),
                size: 1,
                primary: true,
            },
        ]);
        assert_eq!(
            select_primary_file(&version).unwrap().filename,
            "primary.jar"
        );

        let fallback = version_with_files(vec![VersionFile {
            url: "https://example.test/first.jar".to_owned(),
            filename: "first.jar".to_owned(),
            size: 1,
            primary: false,
        }]);
        assert_eq!(
            select_primary_file(&fallback).unwrap().filename,
            "first.jar"
        );
    }

    #[tokio::test]
    async fn content_download_skips_an_existing_filename() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("example.jar");
        std::fs::write(&path, b"existing").unwrap();
        let version = version_with_files(vec![VersionFile {
            url: "https://example.test/example.jar".to_owned(),
            filename: "example.jar".to_owned(),
            size: 1,
            primary: true,
        }]);

        let outcome =
            download_version_file(&crate::net::HttpClient::new(), &version, directory.path())
                .await
                .unwrap();

        assert_eq!(outcome, DownloadOutcome::SkippedExisting(path));
        assert_eq!(
            std::fs::read(directory.path().join("example.jar")).unwrap(),
            b"existing"
        );
    }

    #[tokio::test]
    async fn staged_update_replaces_the_old_file_and_cleans_its_backup() {
        let directory = tempfile::tempdir().unwrap();
        let installed = directory.path().join("example.jar");
        let temporary = directory.path().join(".example.jar.rmcl-download");
        let backup = directory.path().join(".example.jar.rmcl-backup");
        std::fs::write(&installed, b"old version").unwrap();
        std::fs::write(&temporary, b"new version").unwrap();

        replace_installed_file(&temporary, &installed, &installed, &backup)
            .await
            .unwrap();

        assert_eq!(std::fs::read(installed).unwrap(), b"new version");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn discovery_search_ignores_new_modrinth_enum_values() {
        let response: DiscoverySearchResponse = serde_json::from_str(
            r#"{
                "hits": [{
                    "project_id": "project-id",
                    "slug": "example",
                    "title": "Example",
                    "description": "Example project",
                    "downloads": 42,
                    "icon_url": null,
                    "client_side": "unknown",
                    "server_side": "unknown"
                }],
                "total_hits": 1
            }"#,
        )
        .unwrap();

        assert_eq!(response.hits.len(), 1);
        assert_eq!(response.hits[0].project_id, "project-id");
        assert_eq!(response.total_hits, 1);
    }

    #[test]
    fn discovery_search_treats_blank_icon_urls_as_missing() {
        let hit: DiscoverySearchHit = serde_json::from_str(
            r#"{
                "project_id": "project-id",
                "slug": "example",
                "title": "Example",
                "description": "Example project",
                "icon_url": "   "
            }"#,
        )
        .unwrap();

        assert!(DiscoveryProject::from(hit).icon_url.is_none());
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
            100,
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

        let mut seen = result
            .projects
            .iter()
            .map(|project| project.id.clone())
            .collect::<std::collections::HashSet<_>>();
        let end = result.total_hits.min(300);
        for offset in (100..end).step_by(100) {
            let next = search_discovery(
                &client,
                DiscoveryKind::Mod,
                "sodium",
                "1.21.1",
                ModLoader::Fabric,
                offset,
                100,
            )
            .await
            .unwrap();
            assert!(!next.projects.is_empty(), "empty page at offset {offset}");
            assert!(
                next.projects
                    .iter()
                    .all(|project| seen.insert(project.id.clone())),
                "duplicate project at offset {offset}"
            );
        }
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
