use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::feedback::progress::{ProgressTask, ProgressTaskHandle};
use crate::instance::InstanceConfig;
use crate::instance::content::manifest::{
    ContentFileRecord, ContentKind, ContentManifest, FileFingerprint, ProviderProject, Resolution,
    fingerprint, fingerprint_metadata,
};
use crate::instance::content::provider::{FingerprintQuery, ProviderRegistry};
use crate::storage::InstancePaths;

#[derive(Debug)]
pub struct ReconcileResult {
    pub instance_name: String,
    pub manifest: ContentManifest,
    pub error: Option<String>,
}

pub static PENDING_RECONCILIATIONS: LazyLock<Arc<Mutex<Vec<ReconcileResult>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(Vec::new())));

#[derive(Clone)]
struct ReconcileJob {
    instance: InstanceConfig,
    instances_dir: PathBuf,
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

pub fn spawn(instance: InstanceConfig, instances_dir: PathBuf, client: crate::net::HttpClient) {
    schedule(instance, instances_dir, client, false);
}

pub fn spawn_after_change(
    instance: InstanceConfig,
    instances_dir: PathBuf,
    client: crate::net::HttpClient,
) {
    schedule(instance, instances_dir, client, true);
}

fn schedule(
    instance: InstanceConfig,
    instances_dir: PathBuf,
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
        crate::feedback::request_redraw();
    }
}

async fn reconcile(job: ReconcileJob, task: &ProgressTask) -> ReconcileResult {
    let ReconcileJob {
        instance,
        instances_dir,
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

    match inventory {
        Ok(Ok((mut inventory, manifest_path))) => {
            let registry = ProviderRegistry::configured(client);
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
                Ok(()) => match save_reconciled_manifest(
                    &manifest_path,
                    &minecraft_dir,
                    inventory.manifest,
                ) {
                    Ok(manifest) => ReconcileResult {
                        instance_name,
                        manifest,
                        error: None,
                    },
                    Err(error) => ReconcileResult {
                        instance_name,
                        manifest: ContentManifest::default(),
                        error: Some(error.to_string()),
                    },
                },
                Err(error) => {
                    let saved = save_reconciled_manifest(
                        &manifest_path,
                        &minecraft_dir,
                        inventory.manifest,
                    );
                    ReconcileResult {
                        instance_name,
                        manifest: saved.unwrap_or_default(),
                        error: Some(error.to_string()),
                    }
                }
            }
        }
        Ok(Err(error)) => ReconcileResult {
            instance_name,
            manifest: ContentManifest::default(),
            error: Some(error.to_string()),
        },
        Err(error) => ReconcileResult {
            instance_name,
            manifest: ContentManifest::default(),
            error: Some(error.to_string()),
        },
    }
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
        let is_directory = metadata.is_dir();
        let metadata_fingerprint = if is_directory {
            directory_fingerprint_metadata(&path)?
        } else {
            fingerprint_metadata(&path)?
        };
        let modified_ns = metadata_fingerprint.modified_ns;
        let existing = previous.record(&relative_path);
        let unchanged = existing.is_some_and(|record| {
            record.fingerprint.size == metadata_fingerprint.size
                && record.fingerprint.modified_ns == modified_ns
        });
        let oversized = !is_directory
            && max_fingerprint_size_mib > 0
            && metadata.len() > max_fingerprint_size_mib.saturating_mul(1024 * 1024);
        let mut record = if unchanged {
            existing.cloned().unwrap()
        } else if is_directory {
            ContentFileRecord {
                relative_path: relative_path.clone(),
                kind,
                enabled,
                fingerprint: metadata_fingerprint,
                resolution: Resolution::Unmatched {
                    checked_at: now,
                    providers: Vec::new(),
                },
                provider_aliases: Vec::new(),
                required_dependencies: Vec::new(),
                automatic_dependency: false,
                cleanup_eligible: false,
            }
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
                provider_aliases: Vec::new(),
                required_dependencies: Vec::new(),
                automatic_dependency: false,
                cleanup_eligible: false,
            }
        } else {
            ContentFileRecord {
                relative_path: relative_path.clone(),
                kind,
                enabled,
                fingerprint: fingerprint(&path)?,
                resolution: Resolution::Pending,
                provider_aliases: Vec::new(),
                required_dependencies: Vec::new(),
                automatic_dependency: false,
                cleanup_eligible: false,
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
        let curseforge_unchecked = crate::net::curseforge::api_key().is_some()
            && provider_was_not_checked(&record.resolution, "curseforge");
        if !is_directory && !oversized && curseforge_unchecked {
            if record.fingerprint.hash("curseforge").is_none() {
                record.fingerprint = fingerprint(&path)?;
            }
            record.resolution = Resolution::Pending;
        }
        let should_query = !is_directory
            && !oversized
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

fn provider_was_not_checked(resolution: &Resolution, provider: &str) -> bool {
    matches!(
        resolution,
        Resolution::Unmatched { providers, .. }
            if !providers.iter().any(|checked| checked == provider)
    )
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
                let preferred = crate::config::SETTINGS.content.preferred_provider();
                if let Some(project) = candidates
                    .iter()
                    .find(|project| project.provider == preferred)
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
            if supported_content_path(kind, &path) {
                files.push((kind, path));
            }
        }
    }
    if let Ok(worlds) = std::fs::read_dir(minecraft_dir.join("saves")) {
        for world in worlds.flatten().filter(|entry| entry.path().is_dir()) {
            let Ok(entries) = std::fs::read_dir(world.path().join("datapacks")) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if supported_content_path(ContentKind::DataPack, &path) {
                    files.push((ContentKind::DataPack, path));
                }
            }
        }
    }
    files.sort_by(|left, right| left.1.cmp(&right.1));
    Ok(files)
}

fn supported_content_path(kind: ContentKind, path: &Path) -> bool {
    if path.is_dir() {
        return matches!(
            kind,
            ContentKind::ResourcePack | ContentKind::Shader | ContentKind::DataPack
        );
    }
    if !path.is_file() {
        return false;
    }
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    match kind {
        ContentKind::Mod => name.ends_with(".jar") || name.ends_with(".jar.disabled"),
        ContentKind::ResourcePack | ContentKind::Shader | ContentKind::DataPack => {
            name.ends_with(".zip") || name.ends_with(".zip.disabled")
        }
    }
}

fn directory_fingerprint_metadata(path: &Path) -> Result<FileFingerprint, std::io::Error> {
    fn accumulate(path: &Path, size: &mut u64, modified_ns: &mut u128) -> std::io::Result<()> {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            let modified = metadata
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH)
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            *modified_ns = (*modified_ns).max(modified);
            if metadata.is_dir() {
                accumulate(&entry.path(), size, modified_ns)?;
            } else if metadata.is_file() {
                *size = size.saturating_add(metadata.len());
            }
        }
        Ok(())
    }

    let mut size = 0;
    let mut modified_ns = 0;
    accumulate(path, &mut size, &mut modified_ns)?;
    Ok(FileFingerprint {
        size,
        modified_ns,
        hashes: Default::default(),
    })
}

#[cfg(test)]
#[path = "../tests/content/reconcile.rs"]
mod tests;
