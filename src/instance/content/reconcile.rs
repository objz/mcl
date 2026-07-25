use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::content_provider::{FingerprintQuery, ProviderRegistry};
use crate::instance::InstanceConfig;
use crate::instance::content::manifest::{
    ContentFileRecord, ContentKind, ContentManifest, ProviderProject, Resolution, fingerprint,
    fingerprint_metadata,
};
use crate::storage::{InstancePaths, MetadataPaths};
use crate::tui::progress::{ProgressTask, ProgressTaskHandle};

#[derive(Debug)]
pub struct ReconcileResult {
    pub instance_name: String,
    pub manifest: ContentManifest,
    pub icons: HashMap<(String, String), Vec<u8>>,
    pub error: Option<String>,
}

pub static PENDING_RECONCILIATIONS: LazyLock<Arc<Mutex<Vec<ReconcileResult>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(Vec::new())));

#[derive(Clone)]
struct ReconcileJob {
    instance: InstanceConfig,
    instances_dir: PathBuf,
    meta_dir: PathBuf,
    client: crate::net::HttpClient,
}

#[derive(Default)]
struct ReconcileCoordinator {
    queue: VecDeque<ReconcileJob>,
    scheduled: HashSet<String>,
    rerun: HashSet<String>,
    worker_running: bool,
}

impl ReconcileCoordinator {
    fn enqueue(&mut self, job: ReconcileJob, rerun_if_scheduled: bool) -> bool {
        let instance_name = job.instance.name.clone();
        if !self.scheduled.insert(instance_name.clone()) {
            if rerun_if_scheduled {
                self.rerun.insert(instance_name);
            }
            return false;
        }
        self.queue.push_back(job);
        if self.worker_running {
            false
        } else {
            self.worker_running = true;
            true
        }
    }
}

static RECONCILE_COORDINATOR: LazyLock<Mutex<ReconcileCoordinator>> =
    LazyLock::new(|| Mutex::new(ReconcileCoordinator::default()));

pub fn spawn(
    instance: InstanceConfig,
    instances_dir: PathBuf,
    meta_dir: PathBuf,
    client: crate::net::HttpClient,
) {
    schedule(instance, instances_dir, meta_dir, client, false);
}

pub fn spawn_after_change(
    instance: InstanceConfig,
    instances_dir: PathBuf,
    meta_dir: PathBuf,
    client: crate::net::HttpClient,
) {
    schedule(instance, instances_dir, meta_dir, client, true);
}

fn schedule(
    instance: InstanceConfig,
    instances_dir: PathBuf,
    meta_dir: PathBuf,
    client: crate::net::HttpClient,
    rerun_if_scheduled: bool,
) {
    let start_worker = {
        let Ok(mut coordinator) = RECONCILE_COORDINATOR.lock() else {
            return;
        };
        coordinator.enqueue(
            ReconcileJob {
                instance,
                instances_dir,
                meta_dir,
                client,
            },
            rerun_if_scheduled,
        )
    };
    if start_worker {
        tokio::spawn(reconcile_worker());
    }
}

async fn reconcile_worker() {
    let task = ProgressTask::start("Preparing content index");
    loop {
        let job = {
            let Ok(mut coordinator) = RECONCILE_COORDINATOR.lock() else {
                task.finish();
                return;
            };
            match coordinator.queue.pop_front() {
                Some(job) => job,
                None => {
                    coordinator.worker_running = false;
                    task.finish();
                    return;
                }
            }
        };
        let instance_name = job.instance.name.clone();
        let rerun_job = job.clone();
        let result = reconcile(job, &task).await;
        if let Ok(mut results) = PENDING_RECONCILIATIONS.lock() {
            results.retain(|pending| pending.instance_name != instance_name);
            results.push(result);
        }
        if let Ok(mut coordinator) = RECONCILE_COORDINATOR.lock() {
            if coordinator.rerun.remove(&instance_name) {
                coordinator.queue.push_back(rerun_job);
            } else {
                coordinator.scheduled.remove(&instance_name);
            }
        }
        crate::tui::request_redraw();
    }
}

async fn reconcile(job: ReconcileJob, task: &ProgressTask) -> ReconcileResult {
    let ReconcileJob {
        instance,
        instances_dir,
        meta_dir,
        client,
    } = job;
    let instance_name = instance.name;
    task.set_action(format!("Checking content for {instance_name}"));
    task.set_sub_action("Reading saved content index");
    task.set_progress(0, 1);
    let paths = InstancePaths::new(instances_dir.join(&instance_name));
    let manifest_path = paths.content_manifest();
    let minecraft_dir = paths.minecraft();
    let retry_hours = crate::config::SETTINGS.content.unmatched_retry_hours;
    let max_fingerprint_size_mib = crate::config::SETTINGS.content.max_fingerprint_size_mib;
    let inventory_progress = task.handle();
    let inventory_minecraft_dir = minecraft_dir.clone();
    let inventory = tokio::task::spawn_blocking(move || {
        reconcile_inventory(
            &manifest_path,
            &inventory_minecraft_dir,
            retry_hours,
            max_fingerprint_size_mib,
            &inventory_progress,
        )
        .map(|result| (result, manifest_path))
    })
    .await;

    let result = match inventory {
        Ok(Ok((mut inventory, manifest_path))) => {
            let registry = ProviderRegistry::modrinth(client);
            task.set_action(format!("Identifying content for {instance_name}"));
            task.set_sub_action(format!("{} file(s) need matching", inventory.queries.len()));
            if !inventory.queries.is_empty() {
                task.set_progress(0, inventory.queries.len() as u64);
            }
            let resolution_result =
                resolve_queries(&registry, &inventory.queries, &mut inventory.manifest).await;
            if !inventory.queries.is_empty() {
                task.set_progress(
                    inventory.queries.len() as u64,
                    inventory.queries.len() as u64,
                );
            }
            match resolution_result {
                Ok(()) => {
                    task.set_action(format!("Loading content icons for {instance_name}"));
                    let icon_result = load_project_icons(
                        &registry,
                        &inventory.manifest,
                        &MetadataPaths::new(&meta_dir),
                        &task,
                    )
                    .await;
                    match save_reconciled_manifest(
                        &manifest_path,
                        &minecraft_dir,
                        inventory.manifest,
                    ) {
                        Ok(manifest) => ReconcileResult {
                            instance_name,
                            manifest,
                            icons: icon_result.unwrap_or_default(),
                            error: None,
                        },
                        Err(error) => ReconcileResult {
                            instance_name,
                            manifest: ContentManifest::default(),
                            icons: HashMap::new(),
                            error: Some(error.to_string()),
                        },
                    }
                }
                Err(error) => {
                    let saved = save_reconciled_manifest(
                        &manifest_path,
                        &minecraft_dir,
                        inventory.manifest,
                    );
                    ReconcileResult {
                        instance_name,
                        manifest: saved.unwrap_or_default(),
                        icons: HashMap::new(),
                        error: Some(error.to_string()),
                    }
                }
            }
        }
        Ok(Err(error)) => ReconcileResult {
            instance_name,
            manifest: ContentManifest::default(),
            icons: HashMap::new(),
            error: Some(error.to_string()),
        },
        Err(error) => ReconcileResult {
            instance_name,
            manifest: ContentManifest::default(),
            icons: HashMap::new(),
            error: Some(error.to_string()),
        },
    };
    result
}

fn save_reconciled_manifest(
    manifest_path: &Path,
    minecraft_dir: &Path,
    reconciled: ContentManifest,
) -> Result<ContentManifest, crate::instance::content::manifest::ManifestError> {
    let reconciled_paths = reconciled
        .files
        .iter()
        .map(|record| record.relative_path.clone())
        .collect::<HashSet<_>>();
    ContentManifest::update(manifest_path, |current| {
        current.files.retain(|record| {
            reconciled_paths.contains(&record.relative_path)
                || minecraft_dir.join(&record.relative_path).exists()
        });
        for record in reconciled.files {
            let keep_resolved = current
                .record(&record.relative_path)
                .is_some_and(|existing| {
                    existing.fingerprint == record.fingerprint
                        && matches!(existing.resolution, Resolution::Resolved { .. })
                });
            if !keep_resolved {
                current.upsert(record);
            }
        }
        Ok(current.clone())
    })
}

struct Inventory {
    manifest: ContentManifest,
    queries: Vec<FingerprintQuery>,
}

trait InventoryProgress {
    fn set_sub_action(&self, text: &str);
    fn set_progress(&self, current: u64, total: u64);
}

impl InventoryProgress for ProgressTaskHandle {
    fn set_sub_action(&self, text: &str) {
        ProgressTaskHandle::set_sub_action(self, text);
    }

    fn set_progress(&self, current: u64, total: u64) {
        ProgressTaskHandle::set_progress(self, current, total);
    }
}

fn reconcile_inventory(
    manifest_path: &Path,
    minecraft_dir: &Path,
    unmatched_retry_hours: u64,
    max_fingerprint_size_mib: u64,
    task: &impl InventoryProgress,
) -> Result<Inventory, Box<dyn std::error::Error + Send + Sync>> {
    let previous = ContentManifest::load(manifest_path)?;
    let files = content_files(minecraft_dir)?;
    let file_count = files.len() as u64;
    if file_count == 0 {
        task.set_sub_action("No local content files");
        task.set_progress(1, 1);
    } else {
        task.set_progress(0, file_count);
    }
    let now = chrono::Utc::now().timestamp();
    let retry_seconds = unmatched_retry_hours.saturating_mul(60 * 60) as i64;
    let mut manifest = ContentManifest::default();
    let mut queries = Vec::new();

    for (index, (kind, path)) in files.into_iter().enumerate() {
        task.set_sub_action(
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("content"),
        );
        let relative_path = path.strip_prefix(minecraft_dir)?.to_path_buf();
        let enabled = !path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".disabled"));
        let metadata = std::fs::metadata(&path)?;
        let modified_ns = metadata
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let existing = previous.record(&relative_path);
        let unchanged = existing.is_some_and(|record| {
            record.fingerprint.size == metadata.len()
                && record.fingerprint.modified_ns == modified_ns
        });
        let oversized = max_fingerprint_size_mib > 0
            && metadata.len() > max_fingerprint_size_mib.saturating_mul(1024 * 1024);
        let mut record = if unchanged {
            existing.cloned().unwrap()
        } else if oversized {
            tracing::debug!(
                "Skipping automatic provider matching for {} ({} bytes exceeds {} MiB limit)",
                path.display(),
                metadata.len(),
                max_fingerprint_size_mib
            );
            ContentFileRecord {
                relative_path: relative_path.clone(),
                kind,
                enabled,
                fingerprint: fingerprint_metadata(&path)?,
                resolution: Resolution::Unmatched {
                    checked_at: now,
                    providers: Vec::new(),
                },
            }
        } else {
            ContentFileRecord {
                relative_path: relative_path.clone(),
                kind,
                enabled,
                fingerprint: fingerprint(&path)?,
                resolution: Resolution::Pending,
            }
        };
        record.kind = kind;
        record.enabled = enabled;

        if oversized && matches!(record.resolution, Resolution::Pending) {
            record.resolution = Resolution::Unmatched {
                checked_at: now,
                providers: Vec::new(),
            };
        }
        let should_query = !oversized
            && match &record.resolution {
                Resolution::Pending => true,
                Resolution::Unmatched { checked_at, .. } => {
                    now.saturating_sub(*checked_at) >= retry_seconds
                }
                Resolution::Resolved { .. } | Resolution::Ambiguous { .. } => false,
            };
        if should_query {
            record.resolution = Resolution::Pending;
            queries.push(FingerprintQuery {
                key: relative_path.to_string_lossy().into_owned(),
                kind,
                fingerprint: record.fingerprint.clone(),
            });
        }
        manifest.upsert(record);
        task.set_progress(index as u64 + 1, file_count);
    }
    Ok(Inventory { manifest, queries })
}

async fn resolve_queries(
    registry: &ProviderRegistry,
    queries: &[FingerprintQuery],
    manifest: &mut ContentManifest,
) -> Result<(), crate::net::NetError> {
    if queries.is_empty() {
        return Ok(());
    }
    let mut matches: HashMap<String, Vec<ProviderProject>> = HashMap::new();
    let mut checked = Vec::new();
    let mut last_error = None;
    for provider in registry.providers() {
        let provider_id = provider.id();
        match tokio::time::timeout(
            std::time::Duration::from_secs(10),
            provider.resolve_files(queries),
        )
        .await
        {
            Ok(Ok(resolved_files)) => {
                checked.push(provider_id.to_owned());
                for resolved in resolved_files {
                    matches
                        .entry(resolved.key)
                        .or_default()
                        .push(resolved.project);
                }
            }
            Ok(Err(error)) => {
                tracing::warn!(
                    "Skipping {provider_id} content matching after provider error: {error}"
                );
                last_error = Some(error.to_string());
            }
            Err(_) => {
                tracing::warn!(
                    "Skipping {provider_id} content matching after the 10 second timeout"
                );
                last_error = Some(format!("{provider_id} content matching timed out"));
            }
        }
    }
    if checked.is_empty() {
        return Err(crate::net::NetError::TaskFailed(last_error.unwrap_or_else(
            || "No content providers are available".to_owned(),
        )));
    }
    let query_keys = queries
        .iter()
        .map(|query| query.key.as_str())
        .collect::<HashSet<_>>();
    let checked_at = chrono::Utc::now().timestamp();
    for record in &mut manifest.files {
        let key = record.relative_path.to_string_lossy();
        if !query_keys.contains(key.as_ref()) {
            continue;
        }
        let candidates = matches.remove(key.as_ref()).unwrap_or_default();
        record.resolution = match candidates.as_slice() {
            [] => Resolution::Unmatched {
                checked_at,
                providers: checked.clone(),
            },
            [project] => Resolution::Resolved {
                project: project.clone(),
            },
            _ if !crate::config::SETTINGS.content.ask_on_provider_conflict => {
                let preferred = &crate::config::SETTINGS.content.preferred_provider;
                if let Some(project) = candidates
                    .iter()
                    .find(|project| &project.provider == preferred)
                {
                    Resolution::Resolved {
                        project: project.clone(),
                    }
                } else {
                    Resolution::Ambiguous { candidates }
                }
            }
            _ => Resolution::Ambiguous { candidates },
        };
    }
    Ok(())
}

async fn load_project_icons(
    registry: &ProviderRegistry,
    manifest: &ContentManifest,
    metadata: &MetadataPaths,
    task: &ProgressTask,
) -> Result<HashMap<(String, String), Vec<u8>>, crate::net::NetError> {
    let mut projects = manifest
        .files
        .iter()
        .filter_map(|record| match &record.resolution {
            Resolution::Resolved { project } => {
                Some((project.provider.clone(), project.project_id.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    projects.sort();
    projects.dedup();
    let project_count = projects.len() as u64;
    task.set_sub_action(format!("{} icon(s)", projects.len()));
    if project_count > 0 {
        task.set_progress(0, project_count);
    }
    let mut icons = HashMap::new();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    for (index, (provider_id, project_id)) in projects.into_iter().enumerate() {
        let Some(provider) = registry.preferred(&provider_id) else {
            task.set_progress(index as u64 + 1, project_count);
            continue;
        };
        let icon_path = metadata
            .provider_icons(&provider_id)
            .join(format!("{project_id}.img"));
        if let Ok(bytes) = tokio::fs::read(&icon_path).await {
            icons.insert((provider_id, project_id), bytes);
            task.set_progress(index as u64 + 1, project_count);
            continue;
        }
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            tracing::debug!(
                "Content icon loading reached its 10 second startup budget; remaining icons will use fallbacks"
            );
            task.set_progress(project_count, project_count);
            break;
        }
        task.set_sub_action(&project_id);
        let cached_project = metadata
            .provider_projects(&provider_id)
            .join(format!("{project_id}.json"));
        let project_result = match tokio::fs::read(&cached_project).await {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|error| crate::net::NetError::Parse(error.to_string())),
            Err(_) => match tokio::time::timeout(
                remaining.min(std::time::Duration::from_secs(3)),
                provider.project(&project_id),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => {
                    tracing::debug!(
                        "Timed out loading {provider_id} project {project_id}; using fallback"
                    );
                    task.set_progress(index as u64 + 1, project_count);
                    continue;
                }
            },
        };
        match project_result {
            Ok(project) => {
                if let Err(error) = cache_project(metadata, &provider_id, &project) {
                    tracing::warn!("Failed to cache {provider_id} project {project_id}: {error}");
                }
                if let Some(url) = project.icon_url.as_deref() {
                    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                    match tokio::time::timeout(
                        remaining.min(std::time::Duration::from_secs(3)),
                        provider.icon(url),
                    )
                    .await
                    {
                        Ok(Ok(bytes)) => {
                            let cache_result = async {
                                if let Some(parent) = icon_path.parent() {
                                    tokio::fs::create_dir_all(parent).await?;
                                }
                                tokio::fs::write(&icon_path, &bytes).await
                            }
                            .await;
                            if let Err(error) = cache_result {
                                tracing::warn!(
                                    "Failed to cache icon for {provider_id} project \
                                     {project_id}: {error}"
                                );
                            }
                            icons.insert((provider_id, project_id), bytes);
                        }
                        Ok(Err(error)) => tracing::warn!(
                            "Failed to download icon for {provider_id} project \
                             {project_id}: {error}"
                        ),
                        Err(_) => tracing::debug!(
                            "Timed out loading icon for {provider_id} project {project_id}; using fallback"
                        ),
                    }
                }
            }
            Err(error) => {
                tracing::warn!("Failed to load {provider_id} project {project_id}: {error}")
            }
        }
        task.set_progress(index as u64 + 1, project_count);
    }
    Ok(icons)
}

fn cache_project(
    metadata: &MetadataPaths,
    provider: &str,
    project: &crate::net::modrinth::ProjectInfo,
) -> Result<(), crate::net::NetError> {
    let path = metadata
        .provider_projects(provider)
        .join(format!("{}.json", project.id));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::storage::write_atomic(
        &path,
        &serde_json::to_vec_pretty(project)
            .map_err(|error| crate::net::NetError::Parse(error.to_string()))?,
    )?;
    Ok(())
}

fn content_files(minecraft_dir: &Path) -> std::io::Result<Vec<(ContentKind, PathBuf)>> {
    let mut files = Vec::new();
    for kind in [
        ContentKind::Mod,
        ContentKind::ResourcePack,
        ContentKind::Shader,
    ] {
        let directory = minecraft_dir.join(kind.directory());
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && supported_file(kind, &path) {
                files.push((kind, path));
            }
        }
    }
    files.sort_by(|left, right| left.1.cmp(&right.1));
    Ok(files)
}

fn supported_file(kind: ContentKind, path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    match kind {
        ContentKind::Mod => name.ends_with(".jar") || name.ends_with(".jar.disabled"),
        ContentKind::ResourcePack | ContentKind::Shader => {
            name.ends_with(".zip") || name.ends_with(".zip.disabled")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopProgress;

    impl InventoryProgress for NoopProgress {
        fn set_sub_action(&self, _text: &str) {}

        fn set_progress(&self, _current: u64, _total: u64) {}
    }

    fn job(name: &str) -> ReconcileJob {
        ReconcileJob {
            instance: InstanceConfig {
                name: name.to_owned(),
                game_version: "1.21.1".to_owned(),
                loader: crate::instance::ModLoader::Fabric,
                loader_version: None,
                created: chrono::Utc::now(),
                last_played: None,
                java_path: None,
                memory_max: None,
                memory_min: None,
                jvm_args: Vec::new(),
                resolution: None,
                config_sync_profile: None,
            },
            instances_dir: PathBuf::new(),
            meta_dir: PathBuf::new(),
            client: crate::net::HttpClient::new(),
        }
    }

    #[test]
    fn coordinator_queues_instances_once_and_preserves_order() {
        let mut coordinator = ReconcileCoordinator::default();
        assert!(coordinator.enqueue(job("one"), false));
        assert!(!coordinator.enqueue(job("two"), false));
        assert!(!coordinator.enqueue(job("one"), false));
        assert!(!coordinator.rerun.contains("one"));
        assert!(!coordinator.enqueue(job("one"), true));
        assert_eq!(coordinator.queue.len(), 2);
        assert!(coordinator.rerun.contains("one"));
        assert_eq!(coordinator.queue.pop_front().unwrap().instance.name, "one");
        assert_eq!(coordinator.queue.pop_front().unwrap().instance.name, "two");
    }

    #[test]
    fn oversized_content_is_kept_without_hashing_or_provider_query() {
        let temp = tempfile::tempdir().unwrap();
        let minecraft = temp.path().join("minecraft");
        let resource_packs = minecraft.join("resourcepacks");
        std::fs::create_dir_all(&resource_packs).unwrap();
        let pack = resource_packs.join("large.zip");
        let file = std::fs::File::create(&pack).unwrap();
        file.set_len(2 * 1024 * 1024).unwrap();
        let manifest_path = temp.path().join("manifest.json");

        let inventory =
            reconcile_inventory(&manifest_path, &minecraft, 24, 1, &NoopProgress).unwrap();

        assert!(inventory.queries.is_empty());
        assert_eq!(inventory.manifest.files.len(), 1);
        assert!(inventory.manifest.files[0].fingerprint.hashes.is_empty());
        assert!(matches!(
            inventory.manifest.files[0].resolution,
            Resolution::Unmatched { .. }
        ));
    }

    #[test]
    fn unchanged_saved_index_reuses_fingerprint_and_skips_provider_query() {
        let temp = tempfile::tempdir().unwrap();
        let minecraft = temp.path().join("minecraft");
        let mods = minecraft.join("mods");
        std::fs::create_dir_all(&mods).unwrap();
        std::fs::write(mods.join("example.jar"), b"example").unwrap();
        let manifest_path = temp.path().join("manifest.json");

        let mut first =
            reconcile_inventory(&manifest_path, &minecraft, 24, 512, &NoopProgress).unwrap();
        assert_eq!(first.queries.len(), 1);
        let fingerprint = first.manifest.files[0].fingerprint.clone();
        first.manifest.files[0].resolution = Resolution::Unmatched {
            checked_at: chrono::Utc::now().timestamp(),
            providers: vec!["modrinth".to_owned()],
        };
        first.manifest.save(&manifest_path).unwrap();

        let second =
            reconcile_inventory(&manifest_path, &minecraft, 24, 512, &NoopProgress).unwrap();
        assert!(second.queries.is_empty());
        assert_eq!(second.manifest.files[0].fingerprint, fingerprint);
    }
}
