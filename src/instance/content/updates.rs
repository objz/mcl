use std::sync::{Arc, LazyLock, Mutex};

use serde::{Deserialize, Serialize};

use crate::instance::{ContentKind, ContentManifest, InstanceConfig, ModLoader, ProviderProject};
use crate::net::modrinth::VersionInfo;

use super::dependencies::{DependencyPlan, InstallRoot};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableUpdate {
    pub installed: ProviderProject,
    #[serde(default)]
    pub current: Option<VersionInfo>,
    pub target: VersionInfo,
    pub kind: ContentKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSnapshot {
    pub game_version: String,
    pub loader: ModLoader,
    #[serde(default)]
    pub inventory: Vec<ProviderProject>,
    pub updates: Vec<AvailableUpdate>,
    pub failures: Vec<UpdateCheckFailure>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckFailure {
    pub installed: ProviderProject,
    pub kind: ContentKind,
    pub reason: String,
}

pub struct PendingUpdateSnapshot {
    pub instance_name: String,
    pub instance_created: chrono::DateTime<chrono::Utc>,
    pub snapshot: UpdateSnapshot,
}

#[derive(Debug, Clone)]
pub struct UpdateRequest {
    pub title: String,
    pub installed_path: std::path::PathBuf,
    pub target_world: Option<std::path::PathBuf>,
    pub update: AvailableUpdate,
}

#[derive(Debug, Clone)]
pub struct PlannedRootUpdate {
    pub title: String,
    pub installed_path: std::path::PathBuf,
    pub current_version: String,
    pub target: VersionInfo,
}

#[derive(Debug, Clone)]
pub struct UpdateConflict {
    pub title: String,
    pub installed_path: std::path::PathBuf,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct BulkUpdatePlan {
    pub dependency_plan: DependencyPlan,
    pub roots: Vec<PlannedRootUpdate>,
    pub conflicts: Vec<UpdateConflict>,
}

pub static PENDING_UPDATE_SNAPSHOTS: LazyLock<Arc<Mutex<Vec<PendingUpdateSnapshot>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(Vec::new())));

impl UpdateSnapshot {
    pub fn load(path: &std::path::Path) -> Option<Self> {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    }

    pub fn applies_to(&self, instance: &InstanceConfig) -> bool {
        self.game_version == instance.game_version && self.loader == instance.loader
    }

    pub fn matches_manifest(&self, manifest: &ContentManifest) -> bool {
        self.inventory == resolved_inventory(manifest)
    }

    pub fn update_for(&self, installed: &ProviderProject) -> Option<&AvailableUpdate> {
        self.updates.iter().find(|update| {
            update.installed.provider == installed.provider
                && update.installed.project_id == installed.project_id
                && update.installed.version_id == installed.version_id
        })
    }
}

pub fn spawn(instance: InstanceConfig, manifest: ContentManifest, path: std::path::PathBuf) {
    tokio::spawn(async move {
        let snapshot = scan(&instance, &manifest).await;
        if let Ok(bytes) = serde_json::to_vec_pretty(&snapshot)
            && let Err(error) = crate::storage::write_atomic(&path, &bytes)
        {
            tracing::debug!("Could not cache content update snapshot: {error}");
        }
        if let Ok(mut pending) = PENDING_UPDATE_SNAPSHOTS.lock() {
            pending.push(PendingUpdateSnapshot {
                instance_name: instance.name,
                instance_created: instance.created,
                snapshot,
            });
            crate::feedback::request_redraw();
        }
    });
}

pub async fn scan(instance: &InstanceConfig, manifest: &ContentManifest) -> UpdateSnapshot {
    let mut projects = manifest
        .files
        .iter()
        .filter_map(|record| {
            record
                .resolved_project()
                .cloned()
                .map(|project| (project, record.kind))
        })
        .collect::<Vec<_>>();
    projects.sort_by(|left, right| {
        (&left.0.provider, &left.0.project_id, &left.0.version_id).cmp(&(
            &right.0.provider,
            &right.0.project_id,
            &right.0.version_id,
        ))
    });
    projects.dedup_by(|left, right| {
        left.0.provider == right.0.provider
            && left.0.project_id == right.0.project_id
            && left.0.version_id == right.0.version_id
    });
    let inventory = projects
        .iter()
        .map(|(project, _)| project.clone())
        .collect();

    let slots = Arc::new(tokio::sync::Semaphore::new(8));
    let mut tasks = tokio::task::JoinSet::new();
    for (installed, kind) in projects {
        let slots = slots.clone();
        let game_version = instance.game_version.clone();
        let loader = instance.loader;
        tasks.spawn(async move {
            let result = async {
                let _permit = slots
                    .acquire_owned()
                    .await
                    .map_err(|error| error.to_string())?;
                let registry = crate::instance::content::provider::ProviderRegistry::configured(
                    crate::net::HttpClient::new(),
                );
                let provider = registry.get(&installed.provider).ok_or_else(|| {
                    format!("{} content provider is unavailable", installed.provider)
                })?;
                let versions = provider
                    .compatible_versions(&installed.project_id, kind, &game_version, loader)
                    .await
                    .map_err(|error| error.to_string())?;
                let current = versions
                    .iter()
                    .find(|version| version.id == installed.version_id)
                    .cloned();
                let target = versions.first().cloned().filter(|_| {
                    crate::instance::content::provider::has_newer_compatible_version(
                        &versions,
                        &installed.version_id,
                    )
                });
                Ok::<_, String>(target.map(|target| (current, target)))
            }
            .await;
            (installed, kind, result)
        });
    }

    let mut updates = Vec::new();
    let mut failures = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((installed, kind, Ok(Some((current, target))))) => updates.push(AvailableUpdate {
                installed,
                current,
                target,
                kind,
            }),
            Ok((_, _, Ok(None))) => {}
            Ok((installed, kind, Err(reason))) => failures.push(UpdateCheckFailure {
                installed,
                kind,
                reason,
            }),
            Err(error) => tracing::debug!("Content update task failed: {error}"),
        }
    }
    updates.sort_by(|left, right| {
        (&left.installed.provider, &left.installed.project_id)
            .cmp(&(&right.installed.provider, &right.installed.project_id))
    });
    UpdateSnapshot {
        game_version: instance.game_version.clone(),
        loader: instance.loader,
        inventory,
        updates,
        failures,
    }
}

fn resolved_inventory(manifest: &ContentManifest) -> Vec<ProviderProject> {
    let mut inventory = manifest
        .files
        .iter()
        .filter_map(|record| record.resolved_project().cloned())
        .collect::<Vec<_>>();
    inventory.sort_by(|left, right| {
        (&left.provider, &left.project_id, &left.version_id).cmp(&(
            &right.provider,
            &right.project_id,
            &right.version_id,
        ))
    });
    inventory.dedup();
    inventory
}

pub async fn plan_bulk(
    instance: &InstanceConfig,
    manifest: &ContentManifest,
    minecraft_dir: &std::path::Path,
    requests: Vec<UpdateRequest>,
    mut conflicts: Vec<UpdateConflict>,
) -> BulkUpdatePlan {
    let registry = crate::instance::content::provider::ProviderRegistry::configured(
        crate::net::HttpClient::new(),
    );
    let projected_manifest = project_updates(manifest, minecraft_dir, &requests);
    let mut accepted = Vec::new();
    let mut roots = Vec::new();
    for request in requests {
        let root = InstallRoot {
            provider: request.update.installed.provider.clone(),
            project_id: request.update.installed.project_id.clone(),
            title: request.title.clone(),
            version: request.update.target.clone(),
            installed_path: Some(request.installed_path.clone()),
            kind: request.update.kind,
            target_world: request.target_world.clone(),
            force_reinstall: false,
        };
        let mut resolution_manifest = projected_manifest.clone();
        if let Ok(relative_path) = request.installed_path.strip_prefix(minecraft_dir)
            && let Some(current) = manifest.record(relative_path)
            && let Some(projected) = resolution_manifest
                .files
                .iter_mut()
                .find(|record| record.relative_path == relative_path)
        {
            projected.resolution = current.resolution.clone();
        }
        let plan = match super::dependencies::resolve(
            &registry,
            &resolution_manifest,
            minecraft_dir,
            instance,
            root,
        )
        .await
        {
            Ok(plan) => plan,
            Err(error) => {
                conflicts.push(UpdateConflict {
                    title: request.title,
                    installed_path: request.installed_path,
                    reason: error.to_string(),
                });
                continue;
            }
        };
        let mut proposed = accepted.clone();
        proposed.push(plan.clone());
        if let Err(error) = super::dependencies::merge(proposed) {
            conflicts.push(UpdateConflict {
                title: request.title,
                installed_path: request.installed_path,
                reason: error.to_string(),
            });
            continue;
        }
        roots.push(PlannedRootUpdate {
            title: request.title,
            installed_path: request.installed_path,
            current_version: request.update.current.as_ref().map_or_else(
                || request.update.installed.version_id.clone(),
                |version| version.version_number.clone(),
            ),
            target: request.update.target,
        });
        accepted.push(plan);
    }
    BulkUpdatePlan {
        dependency_plan: super::dependencies::merge(accepted).unwrap_or(DependencyPlan {
            items: Vec::new(),
            root_count: 0,
            optional_dependencies: 0,
        }),
        roots,
        conflicts,
    }
}

fn project_updates(
    manifest: &ContentManifest,
    minecraft_dir: &std::path::Path,
    requests: &[UpdateRequest],
) -> ContentManifest {
    let mut projected = manifest.clone();
    for request in requests {
        let Ok(relative_path) = request.installed_path.strip_prefix(minecraft_dir) else {
            continue;
        };
        let Some(record) = projected
            .files
            .iter_mut()
            .find(|record| record.relative_path == relative_path)
        else {
            continue;
        };
        record.resolution = crate::instance::Resolution::Resolved {
            project: ProviderProject {
                provider: request.update.installed.provider.clone(),
                project_id: request.update.installed.project_id.clone(),
                version_id: request.update.target.id.clone(),
            },
        };
    }
    projected
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::modrinth::{VersionFile, VersionType};

    fn version(id: &str) -> VersionInfo {
        VersionInfo {
            id: id.to_owned(),
            project_id: "project".to_owned(),
            name: id.to_owned(),
            version_number: id.to_owned(),
            game_versions: vec!["1.21.1".to_owned()],
            loaders: vec!["fabric".to_owned()],
            version_type: VersionType::Release,
            dependencies: Vec::new(),
            date_published: String::new(),
            files: Vec::<VersionFile>::new(),
        }
    }

    #[test]
    fn cached_updates_match_the_exact_installed_version() {
        let installed = ProviderProject {
            provider: "modrinth".to_owned(),
            project_id: "project".to_owned(),
            version_id: "old".to_owned(),
        };
        let snapshot = UpdateSnapshot {
            game_version: "1.21.1".to_owned(),
            loader: ModLoader::Fabric,
            inventory: vec![installed.clone()],
            updates: vec![AvailableUpdate {
                installed: installed.clone(),
                current: Some(version("old")),
                target: version("new"),
                kind: ContentKind::Mod,
            }],
            failures: Vec::new(),
        };

        assert!(snapshot.update_for(&installed).is_some());
        assert!(
            snapshot
                .update_for(&ProviderProject {
                    version_id: "different".to_owned(),
                    ..installed
                })
                .is_none()
        );
    }

    #[test]
    fn cached_updates_require_the_same_content_inventory() {
        let project = ProviderProject {
            provider: "modrinth".to_owned(),
            project_id: "project".to_owned(),
            version_id: "old".to_owned(),
        };
        let snapshot = UpdateSnapshot {
            game_version: "1.21.1".to_owned(),
            loader: ModLoader::Fabric,
            inventory: vec![project.clone()],
            updates: Vec::new(),
            failures: Vec::new(),
        };
        let mut manifest = ContentManifest::default();
        manifest.files.push(crate::instance::ContentFileRecord {
            relative_path: "mods/example.jar".into(),
            kind: ContentKind::Mod,
            enabled: true,
            fingerprint: crate::instance::FileFingerprint {
                size: 0,
                modified_ns: 0,
                hashes: Default::default(),
            },
            resolution: crate::instance::Resolution::Resolved { project },
            provider_aliases: Vec::new(),
            provider_checks: vec!["modrinth".to_owned()],
            required_dependencies: Vec::new(),
            automatic_dependency: false,
            cleanup_eligible: false,
        });

        assert!(snapshot.matches_manifest(&manifest));
        manifest.files[0].resolution = crate::instance::Resolution::Unmatched {
            checked_at: 0,
            providers: Vec::new(),
        };
        assert!(!snapshot.matches_manifest(&manifest));
    }

    #[test]
    fn bulk_planning_projects_other_selected_updates() {
        let minecraft = std::path::Path::new("/instance/minecraft");
        let installed = ProviderProject {
            provider: "modrinth".to_owned(),
            project_id: "library".to_owned(),
            version_id: "old".to_owned(),
        };
        let mut manifest = ContentManifest::default();
        manifest.files.push(crate::instance::ContentFileRecord {
            relative_path: "mods/library.jar".into(),
            kind: ContentKind::Mod,
            enabled: true,
            fingerprint: crate::instance::FileFingerprint {
                size: 1,
                modified_ns: 1,
                hashes: Default::default(),
            },
            resolution: crate::instance::Resolution::Resolved {
                project: installed.clone(),
            },
            provider_aliases: Vec::new(),
            provider_checks: vec!["modrinth".to_owned()],
            required_dependencies: Vec::new(),
            automatic_dependency: true,
            cleanup_eligible: true,
        });
        let requests = vec![UpdateRequest {
            title: "Library".to_owned(),
            installed_path: minecraft.join("mods/library.jar"),
            target_world: None,
            update: AvailableUpdate {
                installed,
                current: Some(version("old")),
                target: version("new"),
                kind: ContentKind::Mod,
            },
        }];

        let projected = project_updates(&manifest, minecraft, &requests);

        assert_eq!(
            projected.files[0].resolved_project().unwrap().version_id,
            "new"
        );
        assert_eq!(
            manifest.files[0].resolved_project().unwrap().version_id,
            "old"
        );
    }
}
