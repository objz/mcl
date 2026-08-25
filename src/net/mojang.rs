// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

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
use crate::feedback::progress::{clear, set_action, set_progress, set_sub_action};

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
    // os name -> natives classifier (e.g. "linux" -> "natives-linux").
    // present on pre-1.13-era libraries whose native code ships in
    // separate per-platform jars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub natives: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extract: Option<LibraryExtract>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibraryExtract {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LibraryDownloads {
    pub artifact: Option<Artifact>,
    // classifier -> download info (same shape as artifact). populated on
    // libraries that declare `natives`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Artifact {
    pub url: String,
    // empty when upstream omits it; callers fall back to deriving the
    // relative path from the library's maven coordinate.
    #[serde(default)]
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

// sha1 of a file as lowercase hex, or None if unreadable.
fn sha1_hex(path: &Path) -> Option<String> {
    use sha1::Digest;
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = sha1::Sha1::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => hasher.update(&buffer[..read]),
            Err(_) => return None,
        }
    }
    Some(format!("{:x}", hasher.finalize()))
}

// true when the cached file matches mojang's recorded size + sha1.
// guards against partial files left by killed downloads: those used to be
// trusted forever on the strength of an exists() check alone. size is
// checked first so truncated files skip the hashing cost.
fn verify_cached(path: &Path, expected_sha1: &str, expected_size: u64) -> bool {
    if !path.exists() {
        return false;
    }
    if expected_size > 0 {
        let actual = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        if actual != expected_size {
            return false;
        }
    }
    sha1_hex(path).is_some_and(|hash| hash == expected_sha1)
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

    if verify_cached(
        &jar_path,
        &meta.downloads.client.sha1,
        meta.downloads.client.size,
    ) {
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

    // a fresh download that doesn't match the manifest is worse than no
    // download: delete it so the next attempt doesn't trust it.
    if result.is_ok()
        && !verify_cached(
            &jar_path,
            &meta.downloads.client.sha1,
            meta.downloads.client.size,
        )
    {
        let _ = tokio::fs::remove_file(&jar_path).await;
        return Err(NetError::Parse(format!(
            "Downloaded client JAR {} failed its sha1 verification",
            meta.id
        )));
    }
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

    // matches the directory launch passes as java.library.path
    // (instance/launch/mod.rs).
    let natives_dir = crate::storage::MetadataPaths::new(meta_dir)
        .versions()
        .join(&meta.id)
        .join("natives");

    let mut downloads: Vec<(String, PathBuf, String)> = Vec::new();
    // natives jars to unpack once every download has landed:
    // (jar path inside the library cache, extract.exclude prefixes)
    let mut natives_jars: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for library in &meta.libraries {
        if let Some(rules) = &library.rules
            && !crate::launch_profile::rules::evaluate(rules, &rule_ctx)
        {
            tracing::trace!("Skipping library {} due to platform rules", library.name);
            continue;
        }

        if let Some(artifact) = &library.downloads.artifact {
            let rel = match library_relative_path(library, artifact.path.as_str()) {
                Some(rel) => rel,
                None => {
                    tracing::warn!("Skipping unresolvable library {}", library.name);
                    continue;
                }
            };
            let destination = crate::storage::MetadataPaths::new(meta_dir)
                .libraries()
                .join(&rel);
            if verify_cached(&destination, &artifact.sha1, artifact.size) {
                tracing::trace!("Library already cached: {}", rel);
            } else {
                downloads.push((artifact.url.clone(), destination, rel));
            }
        }

        // native classifier jar (pre-1.13 era libraries). the jar itself is
        // cached alongside the other libraries; its contents get unpacked
        // into versions/<id>/natives where java.library.path points.
        if let Some(classifier) = library
            .natives
            .as_ref()
            .and_then(|n| n.get(crate::launch_profile::system::mojang_os_name()))
            && let Some(info) = library
                .downloads
                .classifiers
                .as_ref()
                .and_then(|classifiers| classifiers.get(classifier))
        {
            // the maven fallback must carry the classifier: it selects
            // <artifact>-<version>-<classifier>.jar, not the base jar.
            let rel = if !info.path.is_empty() {
                info.path.clone()
            } else {
                match crate::instance::loader::maven::maven_coord_to_path(&format!(
                    "{}:{}",
                    library.name, classifier
                )) {
                    Some(rel) => rel,
                    None => {
                        tracing::warn!(
                            "Skipping natives of library {}: no usable path",
                            library.name
                        );
                        continue;
                    }
                }
            };
            let destination = crate::storage::MetadataPaths::new(meta_dir)
                .libraries()
                .join(&rel);
            if !verify_cached(&destination, &info.sha1, info.size) {
                downloads.push((info.url.clone(), destination.clone(), rel));
            }
            let exclude = library
                .extract
                .as_ref()
                .and_then(|extract| extract.exclude.clone())
                .unwrap_or_default();
            natives_jars.push((destination, exclude));
        }
    }

    let result = if downloads.is_empty() {
        tracing::info!("All libraries already cached");
        Ok(())
    } else {
        tracing::debug!("Downloading {} missing libraries", downloads.len());
        run_parallel_downloads(client, downloads, false).await
    };
    clear();
    result?;

    for (jar, exclude) in &natives_jars {
        extract_natives(jar, &natives_dir, exclude)?;
    }

    Ok(())
}

// relative path of a library artifact inside the shared library cache.
// upstream usually records it; fall back to deriving it from the maven
// coordinate when the field is missing/empty.
fn library_relative_path(library: &Library, recorded_path: &str) -> Option<String> {
    if !recorded_path.is_empty() {
        return Some(recorded_path.to_owned());
    }
    crate::instance::loader::maven::maven_coord_to_path(&library.name)
}

// unpacks a natives jar into dest. skips directories, archive entries with
// traversal paths, and anything matching an `extract.exclude` prefix.
fn extract_natives(jar: &Path, dest: &Path, exclude: &[String]) -> Result<(), NetError> {
    let file = std::fs::File::open(jar)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| NetError::Parse(format!("Invalid natives jar {}: {e}", jar.display())))?;
    std::fs::create_dir_all(dest)?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| {
            NetError::Parse(format!(
                "Corrupt entry in natives jar {}: {e}",
                jar.display()
            ))
        })?;
        if entry.is_dir() {
            continue;
        }
        // enclosed_name is None for absolute paths / .. traversal
        let Some(rel) = entry.enclosed_name() else {
            tracing::warn!(
                "Skipping unsafe path in natives jar {}: {}",
                jar.display(),
                entry.name()
            );
            continue;
        };
        // zip entry names use '/', but PathBuf renders them with the host
        // separator ('\'), so normalize before matching exclude prefixes.
        let name = rel.to_string_lossy().replace('\\', "/");
        if exclude.iter().any(|prefix| name.starts_with(prefix)) {
            continue;
        }
        let out_path = dest.join(&rel);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    Ok(())
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

        if verify_cached(&destination, &object.hash, object.size) {
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
