// handles all downloads from mojang's servers: version manifests,
// client jars, libraries, and asset objects. this is the core of
// getting vanilla minecraft onto disk.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

use super::{HttpClient, NetError, download_file};
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
    pub rules: Option<Vec<crate::launch_profile::rules::Rule>>,
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
    fetch_version_manifest_from(client, MANIFEST_URL).await
}

// same as fetch_version_manifest but lets the caller pick the URL. exists so
// integration tests can point at a wiremock server; production callers go
// through fetch_version_manifest with the upstream Mojang URL.
pub async fn fetch_version_manifest_from(
    client: &HttpClient,
    url: &str,
) -> Result<VersionManifest, NetError> {
    tracing::debug!("Fetching Mojang version manifest from {}", url);
    let manifest: VersionManifest = client.get_json(url).await?;
    tracing::debug!(
        "Fetched Mojang manifest with {} version(s); latest release={} snapshot={}",
        manifest.versions.len(),
        manifest.latest.release,
        manifest.latest.snapshot
    );
    Ok(manifest)
}

// fetches and parses a version's metadata. also returns the raw response
// bytes so the caller can write the upstream JSON byte-for-byte to disk
// - used by the install path so we don't lose data (e.g. arguments.jvm)
// by re-serializing through our narrow VersionMeta struct.
pub async fn fetch_version_meta_with_raw(
    client: &HttpClient,
    entry: &VersionEntry,
) -> Result<(VersionMeta, Vec<u8>), NetError> {
    tracing::debug!(
        "Fetching Mojang version meta '{}' from {}",
        entry.id,
        entry.url
    );
    client.get_json_with_raw(&entry.url, "version meta").await
}

pub async fn download_client_jar(
    client: &HttpClient,
    meta: &VersionMeta,
    meta_dir: &Path,
) -> Result<(), NetError> {
    let jar_path = crate::storage::MetadataPaths::new(meta_dir)
        .versions()
        .join(&meta.id)
        .join(format!("{}.jar", meta.id));

    if jar_path.exists() {
        tracing::info!("Client JAR already cached: {}", meta.id);
        tracing::trace!("Cached client JAR path: {}", jar_path.display());
        return Ok(());
    }

    set_action(format!("Downloading Minecraft {}...", meta.id));
    tracing::info!(
        "Downloading Minecraft client JAR {} to {}",
        meta.id,
        jar_path.display()
    );

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
    tracing::debug!(
        "Resolving {} libraries for Minecraft {}",
        meta.libraries.len(),
        meta.id
    );

    let features = crate::launch_profile::rules::FeatureSet::default();
    let host_os_version = crate::launch_profile::system::mojang_os_version();
    let rule_ctx = crate::launch_profile::rules::RuleContext {
        os_name: crate::launch_profile::system::mojang_os_name(),
        os_version: &host_os_version,
        arch: crate::launch_profile::system::mojang_arch_name(),
        features: &features,
    };

    let mut downloads = Vec::new();
    for library in &meta.libraries {
        if let Some(rules) = &library.rules
            && !crate::launch_profile::rules::evaluate(rules, &rule_ctx)
        {
            tracing::trace!("Skipping library {} due to platform rules", library.name);
            continue;
        }

        let artifact = match &library.downloads.artifact {
            Some(artifact) => artifact,
            None => {
                tracing::trace!(
                    "Skipping library {} without artifact download",
                    library.name
                );
                continue;
            }
        };

        let destination = crate::storage::MetadataPaths::new(meta_dir)
            .libraries()
            .join(&artifact.path);

        if destination.exists() {
            tracing::trace!("Library already cached: {}", artifact.path);
            continue;
        }

        downloads.push((artifact.url.clone(), destination, artifact.path.clone()));
    }

    if downloads.is_empty() {
        tracing::info!("All libraries already cached");
        clear();
        return Ok(());
    }

    tracing::debug!("Downloading {} missing libraries", downloads.len());
    let result = run_parallel_downloads(client, downloads, false).await;
    clear();
    result
}

pub async fn download_assets(
    client: &HttpClient,
    meta: &VersionMeta,
    meta_dir: &Path,
) -> Result<(), NetError> {
    download_assets_from(client, meta, meta_dir, ASSETS_BASE_URL).await
}

// same as download_assets but lets tests point at a wiremock server for the
// per-asset CDN downloads. the asset index URL still comes from meta.
pub async fn download_assets_from(
    client: &HttpClient,
    meta: &VersionMeta,
    meta_dir: &Path,
    assets_base: &str,
) -> Result<(), NetError> {
    set_action("Downloading assets...");
    let index_path = crate::storage::MetadataPaths::new(meta_dir)
        .assets()
        .join("indexes")
        .join(format!("{}.json", meta.asset_index.id));
    let asset_index: AssetIndexContent = if index_path.exists() {
        let bytes = tokio::fs::read(&index_path).await?;
        serde_json::from_slice(&bytes)
            .map_err(|error| NetError::Parse(format!("Invalid cached asset index: {error}")))?
    } else {
        tracing::debug!(
            "Fetching asset index {} from {}",
            meta.asset_index.id,
            meta.asset_index.url
        );
        let index = match client.get_json(&meta.asset_index.url).await {
            Ok(index) => index,
            Err(e) => {
                clear();
                return Err(e);
            }
        };
        match serde_json::to_string(&index) {
            Ok(json) => {
                if let Some(parent) = index_path.parent() {
                    match tokio::fs::create_dir_all(parent).await {
                        Ok(_) => {}
                        Err(e) => {
                            tracing::debug!("Failed to create asset index dir: {}", e);
                        }
                    }
                }
                match tokio::fs::write(&index_path, json).await {
                    Ok(_) => {
                        tracing::debug!("Saved asset index to {}", index_path.display());
                    }
                    Err(e) => {
                        tracing::debug!(
                            "Failed to write asset index {}: {}",
                            index_path.display(),
                            e
                        );
                    }
                }
            }
            Err(e) => {
                tracing::debug!("Failed to serialize asset index: {}", e);
            }
        }
        index
    };

    // assets are stored by hash with the first 2 chars as a directory prefix,
    // e.g. "ab/ab1234..." - same layout mojang uses on their CDN
    let mut downloads = Vec::new();
    for object in asset_index.objects.values() {
        if object.hash.len() < 2 {
            clear();
            return Err(NetError::Parse(format!(
                "Invalid asset hash: {}",
                object.hash
            )));
        }

        let prefix = &object.hash[..2];
        let url = format!("{}/{}/{}", assets_base, prefix, object.hash);
        let destination = crate::storage::MetadataPaths::new(meta_dir)
            .assets()
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

    tracing::debug!(
        "Downloading {} missing asset(s) from index {}",
        downloads.len(),
        meta.asset_index.id
    );
    let result = run_parallel_downloads(client, downloads, true).await;
    clear();
    result
}

// bounded parallel downloader. spawns up to MAX_CONCURRENT_DOWNLOADS tasks
// and feeds new ones in as each completes. collects errors but keeps going
// so it downloads as much as possible before reporting the first failure.
async fn run_parallel_downloads(
    client: &HttpClient,
    downloads: Vec<(String, PathBuf, String)>,
    report_count_progress: bool,
) -> Result<(), NetError> {
    let total_downloads = downloads.len() as u64;
    tracing::debug!(
        "Starting {} parallel download job(s), max_concurrent={}",
        total_downloads,
        MAX_CONCURRENT_DOWNLOADS
    );
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
                tracing::debug!("Download failed: {}", e);
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
            Err(e) => {
                tracing::debug!("Task panicked: {}", e);
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
        tracing::trace!(
            "Starting parallel download '{}' to {}",
            label,
            destination.display()
        );
        let result = download_file(&task_client, &url, &destination, |_current, _total| {}).await;
        result.map(|()| {
            tracing::trace!("Finished parallel download '{}'", label);
            label
        })
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
