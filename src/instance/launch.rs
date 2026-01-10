use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::instance::models::{InstanceConfig, ModLoader};

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("Version metadata not found: {0}. Re-create the instance to fix this.")]
    MetaNotFound(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0} launch is not yet supported")]
    NotSupported(String),
    #[error("{0}")]
    Auth(String),
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetaJson {
    main_class: String,
    asset_index: MetaAssetIndex,
    libraries: Vec<MetaLibrary>,
}

#[derive(serde::Deserialize)]
struct MetaAssetIndex {
    id: String,
}

#[derive(serde::Deserialize)]
struct MetaLibrary {
    downloads: Option<MetaLibraryDownloads>,
    rules: Option<Vec<MetaRule>>,
}

#[derive(serde::Deserialize)]
struct MetaLibraryDownloads {
    artifact: Option<MetaArtifact>,
}

#[derive(serde::Deserialize)]
struct MetaArtifact {
    path: String,
}

#[derive(serde::Deserialize)]
struct MetaRule {
    action: String,
    os: Option<MetaOsRule>,
}

#[derive(serde::Deserialize)]
struct MetaOsRule {
    name: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoaderProfileJson {
    main_class: String,
    libraries: Vec<LoaderLibrary>,
}

#[derive(serde::Deserialize)]
struct LoaderLibrary {
    name: String,
}

fn lib_allowed(lib: &MetaLibrary) -> bool {
    let rules = match &lib.rules {
        Some(r) => r,
        None => return true,
    };
    let current_os = match std::env::consts::OS {
        "macos" => "osx",
        other => other,
    };
    let mut allowed = false;
    for rule in rules {
        let matches = match &rule.os {
            Some(os) => os.name.as_deref().map(|n| n == current_os).unwrap_or(true),
            None => true,
        };
        if !matches {
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

pub async fn launch(
    config: &InstanceConfig,
    instances_dir: &Path,
    meta_dir: &Path,
) -> Result<(), LaunchError> {
    let name = config.name.clone();
    let instance_dir = instances_dir.join(&name);
    let minecraft_dir = instance_dir.join(".minecraft");

    let meta_path = meta_dir
        .join("versions")
        .join(&config.game_version)
        .join("meta.json");
    if !meta_path.exists() {
        return Err(LaunchError::MetaNotFound(meta_path.display().to_string()));
    }
    let meta: MetaJson = serde_json::from_slice(&std::fs::read(&meta_path)?)?;

    let lib_dir = meta_dir.join("libraries");
    let mut classpath: Vec<PathBuf> = meta
        .libraries
        .iter()
        .filter(|l| lib_allowed(l))
        .filter_map(|l| l.downloads.as_ref()?.artifact.as_ref().map(|a| lib_dir.join(&a.path)))
        .collect();

    let main_class = match config.loader {
        ModLoader::Vanilla => meta.main_class.clone(),
        ModLoader::Fabric => {
            let lv = config.loader_version.as_deref().unwrap_or("unknown");
            let profile_path = meta_dir
                .join("loader-profiles")
                .join(format!("fabric-{}-{}.json", config.game_version, lv));
            if !profile_path.exists() {
                return Err(LaunchError::MetaNotFound(profile_path.display().to_string()));
            }
            let profile: LoaderProfileJson =
                serde_json::from_slice(&std::fs::read(&profile_path)?)?;
            for lib in &profile.libraries {
                if let Some(p) = crate::net::maven_coord_to_path(&lib.name) {
                    classpath.push(lib_dir.join(p));
                }
            }
            profile.main_class
        }
        ModLoader::Quilt => {
            let lv = config.loader_version.as_deref().unwrap_or("unknown");
            let profile_path = meta_dir
                .join("loader-profiles")
                .join(format!("quilt-{}-{}.json", config.game_version, lv));
            if !profile_path.exists() {
                return Err(LaunchError::MetaNotFound(profile_path.display().to_string()));
            }
            let profile: LoaderProfileJson =
                serde_json::from_slice(&std::fs::read(&profile_path)?)?;
            for lib in &profile.libraries {
                if let Some(p) = crate::net::maven_coord_to_path(&lib.name) {
                    classpath.push(lib_dir.join(p));
                }
            }
            profile.main_class
        }
        ModLoader::Forge => {
            let lv = config.loader_version.as_deref().unwrap_or("unknown");
            let profile_path = meta_dir
                .join("loader-profiles")
                .join(format!("forge-{}-{}.json", config.game_version, lv));
            if !profile_path.exists() {
                return Err(LaunchError::MetaNotFound(profile_path.display().to_string()));
            }
            let profile: LoaderProfileJson =
                serde_json::from_slice(&std::fs::read(&profile_path)?)?;
            let forge_lib_dir = minecraft_dir.join("libraries");
            for lib in &profile.libraries {
                if let Some(p) = crate::net::maven_coord_to_path(&lib.name) {
                    let in_meta = lib_dir.join(&p);
                    let in_forge = forge_lib_dir.join(&p);
                    if in_meta.exists() {
                        classpath.push(in_meta);
                    } else if in_forge.exists() {
                        classpath.push(in_forge);
                    }
                }
            }
            profile.main_class
        }
        ModLoader::NeoForge => {
            let lv = config.loader_version.as_deref().unwrap_or("unknown");
            let profile_path = meta_dir
                .join("loader-profiles")
                .join(format!("neoforge-{}.json", lv));
            if !profile_path.exists() {
                return Err(LaunchError::MetaNotFound(profile_path.display().to_string()));
            }
            let profile: LoaderProfileJson =
                serde_json::from_slice(&std::fs::read(&profile_path)?)?;
            let neo_lib_dir = minecraft_dir.join("libraries");
            for lib in &profile.libraries {
                if let Some(p) = crate::net::maven_coord_to_path(&lib.name) {
                    let in_meta = lib_dir.join(&p);
                    let in_neo = neo_lib_dir.join(&p);
                    if in_meta.exists() {
                        classpath.push(in_meta);
                    } else if in_neo.exists() {
                        classpath.push(in_neo);
                    }
                }
            }
            profile.main_class
        }
    };

    classpath.push(
        meta_dir
            .join("versions")
            .join(&config.game_version)
            .join(format!("{}.jar", config.game_version)),
    );

    let sep = if cfg!(windows) { ";" } else { ":" };
    let cp_str = classpath
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(sep);

    let java = config
        .java_path
        .clone()
        .unwrap_or_else(crate::net::detect_java_path);

    let mut jvm: Vec<String> = vec![
        format!("-Xms{}", config.memory_min.as_deref().unwrap_or("512M")),
        format!("-Xmx{}", config.memory_max.as_deref().unwrap_or("2G")),
    ];
    jvm.extend(config.jvm_args.clone());

    let mut account_store = crate::auth::AccountStore::load();
    let (mc_username, mc_uuid, mc_token, mc_user_type) =
        match account_store.active_account().cloned() {
            Some(acc) => {
                let (token, new_refresh) = match acc.account_type {
                    crate::auth::AccountType::Microsoft => {
                        match crate::auth::refresh_and_get_token(&acc).await {
                            Ok(pair) => pair,
                            Err(e) => {
                                return Err(LaunchError::Auth(format!(
                                    "Authentication failed: {e}"
                                )));
                            }
                        }
                    }
                    crate::auth::AccountType::Offline => ("0".to_string(), None),
                };
                if let Some(new_rt) = new_refresh {
                    if let Some(stored) = account_store
                        .accounts
                        .iter_mut()
                        .find(|a| a.uuid == acc.uuid)
                    {
                        stored.refresh_token = Some(new_rt);
                        account_store.save();
                    }
                }
                let user_type = match acc.account_type {
                    crate::auth::AccountType::Microsoft => "msa",
                    crate::auth::AccountType::Offline => "legacy",
                };
                (
                    acc.username.clone(),
                    acc.uuid.clone(),
                    token,
                    user_type.to_string(),
                )
            }
            None => (
                "Player".to_string(),
                "00000000-0000-0000-0000-000000000000".to_string(),
                "0".to_string(),
                "legacy".to_string(),
            ),
        };

    let game_args = vec![
        "--username".to_string(),
        mc_username,
        "--version".to_string(),
        config.game_version.clone(),
        "--gameDir".to_string(),
        minecraft_dir.to_string_lossy().into_owned(),
        "--assetsDir".to_string(),
        meta_dir.join("assets").to_string_lossy().into_owned(),
        "--assetIndex".to_string(),
        meta.asset_index.id.clone(),
        "--uuid".to_string(),
        mc_uuid,
        "--accessToken".to_string(),
        mc_token,
        "--userType".to_string(),
        mc_user_type,
    ];

    let (kill_tx, kill_rx) = tokio::sync::oneshot::channel::<()>();
    crate::running::register_kill(&name, kill_tx);
    crate::running::set_state(&name, crate::running::RunState::Starting);
    tracing::info!(
        "[{}] Starting Minecraft ({} {})",
        name,
        config.game_version,
        config.loader
    );

    let mut cmd = tokio::process::Command::new(&java);
    cmd.args(&jvm);
    cmd.arg("-cp").arg(&cp_str);
    cmd.arg(&main_class);
    cmd.args(&game_args);
    cmd.current_dir(&minecraft_dir);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            crate::running::cleanup_kill_sender(&name);
            crate::running::remove(&name);
            return Err(LaunchError::Io(e));
        }
    };

    crate::running::set_state(&name, crate::running::RunState::Running);

    let log_file_path =
        crate::instance::log_files::create_log_file(instances_dir, &name);

    let name_for_task = name.clone();
    let instances_dir_owned = instances_dir.to_path_buf();
    let meta_dir_owned = meta_dir.to_path_buf();

    tokio::spawn(async move {
        use std::io::Write;
        use std::sync::{Arc, Mutex};
        use tokio::io::AsyncBufReadExt;

        let log_writer: Arc<Mutex<Option<std::fs::File>>> = Arc::new(Mutex::new(
            log_file_path.and_then(|p| std::fs::File::create(p).ok()),
        ));

        if let Some(stdout) = child.stdout.take() {
            let n = name_for_task.clone();
            let w = log_writer.clone();
            let mut lines = tokio::io::BufReader::new(stdout).lines();
            tokio::spawn(async move {
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::info!(target: "mc_instance", "[{}] {}", n, line);
                    crate::instance_logs::push(&n, &line);
                    if let Ok(mut f) = w.lock() {
                        if let Some(f) = f.as_mut() {
                            let _ = writeln!(f, "{}", line);
                        }
                    }
                }
            });
        }

        if let Some(stderr) = child.stderr.take() {
            let n = name_for_task.clone();
            let w = log_writer.clone();
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            tokio::spawn(async move {
                while let Ok(Some(line)) = lines.next_line().await {
                    tracing::warn!(target: "mc_instance", "[{}] {}", n, line);
                    crate::instance_logs::push(&n, &line);
                    if let Ok(mut f) = w.lock() {
                        if let Some(f) = f.as_mut() {
                            let _ = writeln!(f, "[STDERR] {}", line);
                        }
                    }
                }
            });
        }

        let code = tokio::select! {
            _ = kill_rx => {
                tracing::info!("[{}] Kill requested, terminating process", name_for_task);
                let _ = child.kill().await;
                let _ = child.wait().await;
                None
            }
            result = child.wait() => {
                result.ok().and_then(|s| s.code())
            }
        };
        tracing::info!("[{}] Exited with code {:?}", name_for_task, code);

        if code == Some(0) {
            crate::running::remove(&name_for_task);
        } else {
            crate::running::set_state(
                &name_for_task,
                crate::running::RunState::Crashed(code),
            );
        }

        let manager = crate::instance::InstanceManager::new(instances_dir_owned, meta_dir_owned);
        if let Err(e) = manager.touch_last_played(&name_for_task) {
            tracing::warn!("Failed to update last_played for '{}': {}", name_for_task, e);
        }
        crate::running::push_last_played(&name_for_task, chrono::Utc::now());
        crate::running::cleanup_kill_sender(&name_for_task);
    });

    Ok(())
}
