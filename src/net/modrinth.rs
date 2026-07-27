// modrinth api client and provider file downloads.

use serde::Deserialize;
use std::collections::HashMap;

pub use crate::instance::import::mrpack::{
    MrpackFile, MrpackIndex, game_version_from_dependencies, loader_from_dependencies, parse_mrpack,
};

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct ProjectInfo {
    pub id: String,
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct VersionInfo {
    pub id: String,
    #[serde(default)]
    pub project_id: String,
    pub name: String,
    pub version_number: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    #[serde(default)]
    pub date_published: String,
    pub files: Vec<VersionFile>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct VersionFile {
    pub url: String,
    pub filename: String,
    pub size: u64,
    pub primary: bool,
    #[serde(default)]
    pub hashes: HashMap<String, String>,
}

use crate::instance::{ContentKind, ModLoader};

pub type DiscoveryKind = ContentKind;

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
    kind: ContentKind,
    query: &str,
    game_version: &str,
    loader: ModLoader,
    offset: usize,
    limit: usize,
) -> Result<DiscoveryResults, crate::net::NetError> {
    let progress = crate::tui::progress::ProgressTask::start(if query.trim().is_empty() {
        "Loading Modrinth discovery".to_owned()
    } else {
        format!("Searching Modrinth for '{}'", query.trim())
    });
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
    let results: DiscoverySearchResponse = match client.get_json(&url).await {
        Ok(results) => results,
        Err(error) => {
            progress.fail(&error);
            return Err(error);
        }
    };
    progress.finish();

    Ok(DiscoveryResults {
        total_hits: results.total_hits.max(0) as usize,
        projects: results
            .hits
            .into_iter()
            .map(DiscoveryProject::from)
            .collect(),
    })
}

fn discovery_facets(kind: ContentKind, game_version: &str, loader: ModLoader) -> String {
    let mut facets = vec![vec![format!("project_type:{}", project_type(kind))]];
    if !game_version.is_empty() {
        facets.push(vec![format!("versions:{game_version}")]);
    }
    if kind == ContentKind::Mod
        && let Some(loader) = loader_facet(loader)
    {
        facets.push(vec![format!("categories:{loader}")]);
    }
    serde_json::to_string(&facets).unwrap_or_else(|_| "[]".to_string())
}

fn project_type(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Mod => "mod",
        ContentKind::ResourcePack => "resourcepack",
        ContentKind::Shader => "shader",
    }
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
    kind: ContentKind,
    game_version: &str,
    loader: ModLoader,
) -> Result<Vec<VersionInfo>, crate::net::NetError> {
    let progress = crate::tui::progress::ProgressTask::start("Loading compatible project versions");
    let url = content_versions_url(API_BASE, project_id, kind, game_version, loader);
    tracing::debug!(
        "Fetching compatible Modrinth versions for '{}' ({}, {})",
        project_id,
        game_version,
        loader
    );
    match client.get_json(&url).await {
        Ok(versions) => {
            progress.finish();
            Ok(versions)
        }
        Err(error) => {
            progress.fail(&error);
            Err(error)
        }
    }
}

fn content_versions_url(
    api_base: &str,
    project_id: &str,
    kind: ContentKind,
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
    if kind == ContentKind::Mod
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
        if verify_version_file(&path, file)? {
            return Ok(DownloadOutcome::SkippedExisting(path));
        }
        return Err(crate::net::NetError::Parse(format!(
            "Existing file '{}' does not match the selected Modrinth version",
            path.display()
        )));
    }
    let temporary = destination.join(format!(".{}.{}.rmcl-download", file.filename, version.id));
    if temporary.exists() {
        tokio::fs::remove_file(&temporary).await?;
    }
    let progress =
        crate::tui::progress::ProgressTask::start(format!("Downloading {}", file.filename));
    crate::net::download_file(client, &file.url, &temporary, |current, total| {
        progress.set_progress(current, total);
    })
    .await?;
    if !verify_version_file(&temporary, file)? {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(crate::net::NetError::Parse(format!(
            "Downloaded file '{}' failed its size or hash verification",
            file.filename
        )));
    }
    tokio::fs::rename(&temporary, &path).await?;
    progress.finish();
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
    if backup.exists() {
        if !installed_path.exists() {
            tokio::fs::rename(&backup, installed_path).await?;
        } else {
            tokio::fs::remove_file(&backup).await?;
        }
    }
    if temporary.exists() {
        tokio::fs::remove_file(&temporary).await?;
    }

    let progress =
        crate::tui::progress::ProgressTask::start(format!("Downloading {}", file.filename));
    crate::net::download_file(client, &file.url, &temporary, |current, total| {
        progress.set_progress(current, total);
    })
    .await?;
    if !verify_version_file(&temporary, file)? {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(crate::net::NetError::Parse(format!(
            "Downloaded file '{}' failed its size or hash verification",
            file.filename
        )));
    }
    replace_installed_file(&temporary, &target, installed_path, &backup).await?;
    progress.finish();
    Ok(DownloadOutcome::Downloaded(target))
}

fn verify_version_file(
    path: &std::path::Path,
    expected: &VersionFile,
) -> Result<bool, crate::net::NetError> {
    let fingerprint = crate::instance::content::manifest::fingerprint(path)?;
    if expected.size > 0 && fingerprint.size != expected.size {
        return Ok(false);
    }
    for algorithm in ["sha512", "sha1"] {
        if let Some(expected_hash) = expected.hashes.get(algorithm)
            && fingerprint.hash(algorithm) != Some(expected_hash.as_str())
        {
            return Ok(false);
        }
    }
    Ok(true)
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

#[derive(Debug, serde::Serialize)]
struct VersionFilesRequest<'a> {
    hashes: &'a [String],
    algorithm: &'a str,
}

pub async fn resolve_version_files(
    client: &crate::net::HttpClient,
    hashes: &[String],
    algorithm: &str,
) -> Result<HashMap<String, VersionInfo>, crate::net::NetError> {
    if hashes.is_empty() {
        return Ok(HashMap::new());
    }
    client
        .post_json(
            &format!("{API_BASE}/version_files"),
            &VersionFilesRequest { hashes, algorithm },
        )
        .await
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

#[cfg(test)]
mod tests {
    use super::*;

    fn version_with_files(files: Vec<VersionFile>) -> VersionInfo {
        VersionInfo {
            id: "version-id".to_owned(),
            project_id: "project-id".to_owned(),
            name: "Version 1".to_owned(),
            version_number: "1.0.0".to_owned(),
            game_versions: vec!["1.21.1".to_owned()],
            loaders: vec!["fabric".to_owned()],
            date_published: String::new(),
            files,
        }
    }

    #[test]
    fn discovery_mod_facets_include_instance_compatibility() {
        let facets = discovery_facets(ContentKind::Mod, "1.21.1", ModLoader::Fabric);
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
        let facets = discovery_facets(ContentKind::ResourcePack, "1.20.1", ModLoader::Forge);
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
            ContentKind::Mod,
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
            ContentKind::ResourcePack,
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
                hashes: HashMap::new(),
            },
            VersionFile {
                url: "https://example.test/primary.jar".to_owned(),
                filename: "primary.jar".to_owned(),
                size: 1,
                primary: true,
                hashes: HashMap::new(),
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
            hashes: HashMap::new(),
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
            size: b"existing".len() as u64,
            primary: true,
            hashes: HashMap::new(),
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
}
