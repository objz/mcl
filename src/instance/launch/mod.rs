// builds the full java command line and spawns minecraft as a child process.
// handles classpath assembly, auth token injection, and log capture.
// loader-specific patches live in submodules (e.g. patches.rs for lwjgl3ify).

mod patches;

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

// subset of mojang's version meta json, only the bits relevant to launching
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
    #[serde(default)]
    game_arguments: Vec<String>,
}

#[derive(serde::Deserialize)]
struct LoaderLibrary {
    name: String,
}

struct GameAuth {
    username: String,
    uuid: String,
    token: String,
    user_type: String,
}

// mojang's library rules are a fun little state machine: each rule can allow
// or disallow based on OS. if no rule matches the current OS, the library is
// included only if no rule "dominated" (matched at all). yes, it's weird.
fn lib_allowed(lib: &MetaLibrary) -> bool {
    let Some(rules) = &lib.rules else {
        return true;
    };
    let current_os = match std::env::consts::OS {
        "macos" => "osx",
        other => other,
    };
    let mut dominated = false;
    for rule in rules {
        let matches_os = rule
            .os
            .as_ref()
            .and_then(|os| os.name.as_deref())
            .is_none_or(|n| n == current_os);
        if !matches_os {
            continue;
        }
        dominated = true;
        match rule.action.as_str() {
            "disallow" => return false,
            "allow" => return true,
            _ => {}
        }
    }
    !dominated
}

fn build_game_args(
    config: &InstanceConfig,
    minecraft_dir: &Path,
    meta_dir: &Path,
    asset_index_id: &str,
    auth: GameAuth,
    loader_game_args: Vec<String>,
) -> Vec<String> {
    let mut game_args = vec![
        "--username".to_string(),
        auth.username,
        "--version".to_string(),
        config.game_version.clone(),
        "--gameDir".to_string(),
        minecraft_dir.to_string_lossy().into_owned(),
        "--assetsDir".to_string(),
        meta_dir.join("assets").to_string_lossy().into_owned(),
        "--assetIndex".to_string(),
        asset_index_id.to_string(),
        "--uuid".to_string(),
        auth.uuid,
        "--accessToken".to_string(),
        auth.token,
        "--userProperties".to_string(),
        "{}".to_string(),
        "--userType".to_string(),
        auth.user_type,
    ];
    game_args.extend(loader_game_args);
    game_args
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
    let meta: MetaJson = serde_json::from_slice(&tokio::fs::read(&meta_path).await?)?;

    let lib_dir = meta_dir.join("libraries");
    let mut classpath: Vec<PathBuf> = meta
        .libraries
        .iter()
        .filter(|l| lib_allowed(l))
        .filter_map(|l| {
            l.downloads
                .as_ref()?
                .artifact
                .as_ref()
                .map(|a| lib_dir.join(&a.path))
        })
        .collect();

    let lv = config.loader_version.as_deref().unwrap_or("unknown");
    let profile_filename = match config.loader {
        ModLoader::Vanilla => None,
        ModLoader::Fabric => Some(format!("fabric-{}-{}.json", config.game_version, lv)),
        ModLoader::Quilt => Some(format!("quilt-{}-{}.json", config.game_version, lv)),
        ModLoader::Forge => Some(format!("forge-{}-{}.json", config.game_version, lv)),
        ModLoader::NeoForge => Some(format!("neoforge-{}.json", lv)),
    };

    // if there's a mod loader, read its profile to get the real main class,
    // extra libraries, and any additional game arguments (e.g. --tweakClass)
    let (main_class, loader_game_args) = if let Some(filename) = profile_filename {
        let profile_path = meta_dir.join("loader-profiles").join(&filename);
        if !profile_path.exists() {
            return Err(LaunchError::MetaNotFound(
                profile_path.display().to_string(),
            ));
        }
        let profile: LoaderProfileJson =
            serde_json::from_slice(&tokio::fs::read(&profile_path).await?)?;

        // forge/neoforge install some libs locally in the instance dir.
        // local libs take priority so modpacks can ship patched versions
        // (e.g. GTNH's launchwrapper patched for java 9+ compatibility)
        let has_local_libs = matches!(config.loader, ModLoader::Forge | ModLoader::NeoForge);
        let local_lib_dir = minecraft_dir.join("libraries");

        for lib in &profile.libraries {
            if let Some(p) = crate::net::maven_coord_to_path(&lib.name) {
                if has_local_libs {
                    let in_local = local_lib_dir.join(&p);
                    let in_meta = lib_dir.join(&p);
                    if in_local.exists() {
                        classpath.push(in_local);
                    } else if in_meta.exists() {
                        classpath.push(in_meta);
                    }
                } else {
                    classpath.push(lib_dir.join(p));
                }
            }
        }
        (profile.main_class, profile.game_arguments)
    } else {
        (meta.main_class.clone(), Vec::new())
    };

    classpath.push(
        meta_dir
            .join("versions")
            .join(&config.game_version)
            .join(format!("{}.jar", config.game_version)),
    );

    // apply loader-specific patches (lwjgl3ify for old forge on java 9+)
    let (patch_jvm_args, main_class, extra_args) = if matches!(config.loader, ModLoader::Forge) {
        match patches::apply(&minecraft_dir, &lib_dir, &mut classpath).await {
            Some(p) => (p.jvm_args, p.main_class, p.extra_args),
            None => (Vec::new(), main_class, Vec::new()),
        }
    } else {
        (Vec::new(), main_class, Vec::new())
    };

    let sep = if cfg!(windows) { ";" } else { ":" };
    let cp_str = classpath
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(sep);

    // java resolution: instance override > global setting > auto-detect
    let java = config
        .java_path
        .clone()
        .or_else(|| {
            crate::config::SETTINGS
                .paths
                .effective_java_path()
                .map(str::to_owned)
        })
        .unwrap_or_else(crate::net::detect_java_path);

    let mut jvm: Vec<String> = vec![
        format!("-Xms{}", config.memory_min.as_deref().unwrap_or("512M")),
        format!("-Xmx{}", config.memory_max.as_deref().unwrap_or("2G")),
    ];
    jvm.extend(patch_jvm_args);
    jvm.extend(config.jvm_args.clone());

    // resolve auth credentials, refreshing the microsoft token if needed.
    // falls back to a generic offline player if no account is configured.
    let mut account_store = crate::auth::AccountStore::load();
    let (mc_username, mc_uuid, mc_token, mc_user_type) = match account_store
        .active_account()
        .cloned()
    {
        Some(acc) => {
            let (token, new_refresh, new_expires) = match acc.account_type {
                crate::auth::AccountType::Microsoft => {
                    match crate::auth::refresh_and_get_token(&acc).await {
                        Ok(triple) => triple,
                        Err(e) => {
                            return Err(LaunchError::Auth(format!("Authentication failed: {e}")));
                        }
                    }
                }
                crate::auth::AccountType::Offline => ("0".to_string(), None, None),
            };
            if let Some(stored) = account_store
                .accounts
                .iter_mut()
                .find(|a| a.uuid == acc.uuid)
            {
                let mut changed = false;
                if let Some(new_rt) = new_refresh {
                    stored.refresh_token = Some(new_rt);
                    changed = true;
                }
                if let Some(expires) = new_expires {
                    stored.cached_mc_token = Some(token.clone());
                    stored.cached_mc_token_expires_at = Some(expires);
                    changed = true;
                }
                if changed {
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

    let game_args = build_game_args(
        config,
        &minecraft_dir,
        meta_dir,
        &meta.asset_index.id,
        GameAuth {
            username: mc_username,
            uuid: mc_uuid,
            token: mc_token,
            user_type: mc_user_type,
        },
        loader_game_args,
    );

    let (kill_tx, kill_rx) = tokio::sync::oneshot::channel::<()>();
    crate::running::register_kill(&name, kill_tx);
    crate::running::set_state(&name, crate::running::RunState::Starting);
    tracing::info!(
        "[{}] Starting Minecraft ({} {})",
        name,
        config.game_version,
        config.loader
    );

    tracing::info!("[{}] Java: {}", name, java);
    tracing::info!("[{}] JVM args: {:?}", name, jvm);
    tracing::info!(
        "[{}] Classpath:\n{}",
        name,
        classpath
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
    tracing::info!("[{}] Main class: {}", name, main_class);

    let mut cmd = tokio::process::Command::new(&java);
    cmd.args(&jvm);
    cmd.arg("-cp").arg(&cp_str);
    cmd.arg(&main_class);
    cmd.args(&extra_args);
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

    let log_file_path = crate::instance::log_files::create_log_file(instances_dir, &name);

    let name_for_task = name.clone();
    let instances_dir_owned = instances_dir.to_path_buf();
    let meta_dir_owned = meta_dir.to_path_buf();

    // spawn a background task to babysit the child process: capture stdout/stderr
    // into both the TUI log viewer and a timestamped log file on disk
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
                    if let Ok(mut f) = w.lock()
                        && let Some(f) = f.as_mut()
                    {
                        let _ = writeln!(f, "{}", line);
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
                    if let Ok(mut f) = w.lock()
                        && let Some(f) = f.as_mut()
                    {
                        let _ = writeln!(f, "[STDERR] {}", line);
                    }
                }
            });
        }

        // wait for either the process to exit naturally or a kill signal from the TUI
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
            crate::running::set_state(&name_for_task, crate::running::RunState::Crashed(code));
        }

        let manager = crate::instance::InstanceManager::new(instances_dir_owned, meta_dir_owned);
        if let Err(e) = manager.touch_last_played(&name_for_task) {
            tracing::warn!(
                "Failed to update last_played for '{}': {}",
                name_for_task,
                e
            );
        }
        crate::running::push_last_played(&name_for_task, chrono::Utc::now());
        crate::running::cleanup_kill_sender(&name_for_task);
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn test_config() -> InstanceConfig {
        InstanceConfig {
            name: "test".to_owned(),
            game_version: "1.7.10".to_owned(),
            loader: ModLoader::Forge,
            loader_version: Some("10.13.4.1614".to_owned()),
            created: Utc::now(),
            last_played: None,
            java_path: None,
            memory_max: None,
            memory_min: None,
            jvm_args: Vec::new(),
            resolution: None,
        }
    }

    #[test]
    fn game_args_include_empty_user_properties() {
        let args = build_game_args(
            &test_config(),
            Path::new("/instances/test/.minecraft"),
            Path::new("/meta"),
            "legacy",
            GameAuth {
                username: "TestPlayer".to_owned(),
                uuid: "00000000-0000-0000-0000-000000000000".to_owned(),
                token: "token".to_owned(),
                user_type: "msa".to_owned(),
            },
            vec![
                "--tweakClass".to_owned(),
                "cpw.mods.fml.common.launcher.FMLTweaker".to_owned(),
            ],
        );

        let position = args
            .iter()
            .position(|arg| arg == "--userProperties")
            .expect("game args should include --userProperties");
        assert_eq!(args.get(position + 1).map(String::as_str), Some("{}"));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--tweakClass", "cpw.mods.fml.common.launcher.FMLTweaker"])
        );
    }
}
