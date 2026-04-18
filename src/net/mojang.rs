use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use super::{download_file, HttpClient, NetError};
use crate::tui::progress::{clear, set_action, set_progress, set_sub_action};

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const ASSETS_BASE_URL: &str = "https://resources.download.minecraft.net";
const MAX_CONCURRENT_DOWNLOADS: usize = 10;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionManifest {
    pub latest: LatestVersions,
    pub versions: Vec<VersionEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LatestVersions {
    pub release: String,
    pub snapshot: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub version_type: String,
    pub url: String,
    pub sha1: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionMeta {
    pub id: String,
    pub main_class: String,
    pub asset_index: AssetIndex,
    pub downloads: VersionDownloads,
    pub libraries: Vec<Library>,
    pub java_version: Option<JavaVersion>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetIndex {
    pub id: String,
    pub url: String,
    pub sha1: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VersionDownloads {
    pub client: Download,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Download {
    pub url: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Library {
    pub name: String,
    pub downloads: LibraryDownloads,
    pub rules: Option<Vec<Rule>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibraryDownloads {
    pub artifact: Option<Artifact>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Artifact {
    pub url: String,
    pub path: String,
    pub sha1: String,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Rule {
    pub action: String,
    pub os: Option<OsRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OsRule {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaVersion {
    pub major_version: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetIndexContent {
    pub objects: HashMap<String, AssetObject>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AssetObject {
    pub hash: String,
    pub size: u64,
}

pub async fn fetch_version_manifest(client: &HttpClient) -> Result<VersionManifest, NetError> {
    client.get_json(MANIFEST_URL).await
}

pub async fn fetch_version_meta(
    client: &HttpClient,
    entry: &VersionEntry,
) -> Result<VersionMeta, NetError> {
    client.get_json(&entry.url).await
}

pub async fn download_client_jar(
    client: &HttpClient,
    meta: &VersionMeta,
    meta_dir: &Path,
) -> Result<(), NetError> {
    let jar_path = meta_dir
        .join("versions")
        .join(&meta.id)
        .join(format!("{}.jar", meta.id));

    if jar_path.exists() {
        tracing::info!("Client JAR already cached: {}", meta.id);
        return Ok(());
    }

    set_action(format!("Downloading Minecraft {}...", meta.id));

    let result = download_file(
        client,
        &meta.downloads.client.url,
        &jar_path,
        |current, total| {
            set_progress(current, total);
        },
    )
    .await;

    clear();
    result
}

pub async fn download_libraries(
    client: &HttpClient,
    meta: &VersionMeta,
    meta_dir: &Path,
) -> Result<(), NetError> {
    set_action("Downloading libraries...");

    let mut downloads = Vec::new();
    for library in &meta.libraries {
        if !library_allowed_for_current_os(library) {
            continue;
        }

        let artifact = match &library.downloads.artifact {
            Some(artifact) => artifact,
            None => continue,
        };

        let destination = meta_dir.join("libraries").join(&artifact.path);

        if destination.exists() {
            continue;
        }

        downloads.push((artifact.url.clone(), destination, artifact.path.clone()));
    }

    if downloads.is_empty() {
        tracing::info!("All libraries already cached");
        clear();
        return Ok(());
    }

    let result = run_parallel_downloads(client, downloads, false).await;
    clear();
    result
}

pub async fn download_assets(
    client: &HttpClient,
    meta: &VersionMeta,
    meta_dir: &Path,
) -> Result<(), NetError> {
    set_action("Downloading assets...");

    let asset_index: AssetIndexContent = match client.get_json(&meta.asset_index.url).await {
        Ok(index) => index,
        Err(e) => {
            clear();
            return Err(e);
        }
    };

    let index_path = meta_dir
        .join("assets")
        .join("indexes")
        .join(format!("{}.json", meta.asset_index.id));
    if !index_path.exists() {
        match serde_json::to_string(&asset_index) {
            Ok(json) => {
                if let Some(parent) = index_path.parent() {
                    match tokio::fs::create_dir_all(parent).await {
                        Ok(_) => {}
                        Err(e) => {
                            tracing::error!("Failed to create asset index dir: {}", e);
                        }
                    }
                }
                match tokio::fs::write(&index_path, json).await {
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!("Failed to write asset index: {}", e);
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to serialize asset index: {}", e);
            }
        }
    }

    let mut downloads = Vec::new();
    for object in asset_index.objects.values() {
        if object.hash.len() < 2 {
            clear();
            return Err(NetError::Parse(format!("Invalid asset hash: {}", object.hash)));
        }

        let prefix = &object.hash[..2];
        let url = format!("{}/{}/{}", ASSETS_BASE_URL, prefix, object.hash);
        let destination = meta_dir
            .join("assets")
            .join("objects")
            .join(prefix)
            .join(&object.hash);

        if destination.exists() {
            continue;
        }

        downloads.push((url, destination, object.hash.clone()));
    }

    if downloads.is_empty() {
        tracing::info!("All assets already cached");
        clear();
        return Ok(());
    }

    let result = run_parallel_downloads(client, downloads, true).await;
    clear();
    result
}

fn library_allowed_for_current_os(library: &Library) -> bool {
    let rules = match &library.rules {
        Some(rules) => rules,
        None => return true,
    };

    let current_os = mojang_os_name();
    let mut allowed = false;

    for rule in rules {
        let matches_os = match &rule.os {
            Some(os_rule) => match &os_rule.name {
                Some(name) => name == current_os,
                None => true,
            },
            None => true,
        };

        if !matches_os {
            continue;
        }

        if rule.action == "disallow" {
            return false;
        }

        if rule.action == "allow" {
            allowed = true;
        }
    }

    allowed
}

fn mojang_os_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "osx",
        other => other,
    }
}

async fn run_parallel_downloads(
    client: &HttpClient,
    downloads: Vec<(String, PathBuf, String)>,
    report_count_progress: bool,
) -> Result<(), NetError> {
    let total_downloads = downloads.len() as u64;
    let completed = Arc::new(AtomicU64::new(0));
    let mut queue = downloads.into_iter();
    let mut set = JoinSet::new();

    for _ in 0..MAX_CONCURRENT_DOWNLOADS {
        let next_job = match queue.next() {
            Some(job) => job,
            None => break,
        };

        spawn_download_task(&mut set, client, next_job);
    }

    let mut first_error: Option<NetError> = None;

    while let Some(join_result) = set.join_next().await {
        match join_result {
            Ok(Ok(label)) => {
                let finished = completed.fetch_add(1, Ordering::SeqCst) + 1;
                if report_count_progress {
                    set_progress(finished, total_downloads);
                }
                set_sub_action(label);
            }
            Ok(Err(e)) => {
                tracing::error!("Download failed: {}", e);
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
            Err(e) => {
                tracing::error!("Task panicked: {}", e);
                if first_error.is_none() {
                    first_error = Some(NetError::TaskFailed(format!("Join error: {}", e)));
                }
            }
        }

        let next_job = match queue.next() {
            Some(job) => job,
            None => continue,
        };

        spawn_download_task(&mut set, client, next_job);
    }

    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn spawn_download_task(
    set: &mut JoinSet<Result<String, NetError>>,
    client: &HttpClient,
    job: (String, PathBuf, String),
) {
    let (url, destination, label) = job;
    let task_client = client.clone();

    set.spawn(async move {
        let result = download_file(&task_client, &url, &destination, |_current, _total| {}).await;
        result.map(|()| label)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::HttpClient;

    #[tokio::test]
    #[ignore = "hits live Mojang API"]
    async fn test_fetch_manifest_contains_1_20_1() {
        let client = HttpClient::new();
        match fetch_version_manifest(&client).await {
            Ok(manifest) => {
                let found = manifest.versions.iter().any(|v| v.id == "1.20.1");
                assert!(found, "1.20.1 should be in the manifest");
            }
            Err(e) => panic!("fetch_version_manifest failed: {}", e),
        }
    }
}
