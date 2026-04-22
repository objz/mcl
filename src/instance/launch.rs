// builds the full java command line and spawns minecraft as a child process.
// handles classpath assembly, auth token injection, and log capture.

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

    // lwjgl3ify ships a patched launchwrapper and retrofuturabootstrap for
    // java 9+ compat. if present, prepend its patches to the classpath,
    // override the main class to use RFB's bootstrap, and add the required
    // --add-opens and system classloader flags.
    let (lwjgl3ify_jvm_args, main_class) = if matches!(config.loader, ModLoader::Forge) {
        let (jvm_args, override_main) = apply_lwjgl3ify_patches(&minecraft_dir, &mut classpath);
        (jvm_args, override_main.unwrap_or(main_class))
    } else {
        (Vec::new(), main_class)
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
    jvm.extend(lwjgl3ify_jvm_args);
    jvm.extend(config.jvm_args.clone());

    // resolve auth credentials, refreshing the microsoft token if needed.
    // falls back to a generic offline player if no account is configured.
    let mut account_store = crate::auth::AccountStore::load();
    let (mc_username, mc_uuid, mc_token, mc_user_type) = match account_store
        .active_account()
        .cloned()
    {
        Some(acc) => {
            let (token, new_refresh) = match acc.account_type {
                crate::auth::AccountType::Microsoft => {
                    match crate::auth::refresh_and_get_token(&acc).await {
                        Ok(pair) => pair,
                        Err(e) => {
                            return Err(LaunchError::Auth(format!("Authentication failed: {e}")));
                        }
                    }
                }
                crate::auth::AccountType::Offline => ("0".to_string(), None),
            };
            if let Some(new_rt) = new_refresh
                && let Some(stored) = account_store
                    .accounts
                    .iter_mut()
                    .find(|a| a.uuid == acc.uuid)
                {
                    stored.refresh_token = Some(new_rt);
                    account_store.save();
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

    let mut game_args = vec![
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
    game_args.extend(loader_game_args);

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
                        && let Some(f) = f.as_mut() {
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
                        && let Some(f) = f.as_mut() {
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

// lwjgl3ify (used by GTNH and other old forge modpacks) bundles a
// forgePatches.zip with patched launchwrapper classes that fix the
// URLClassLoader cast on java 9+. if we find it, extract the patches
// and prepend them to the classpath so they shadow the vanilla classes.
// also parses Add-Opens from the zip's manifest for the required
// --add-opens flags.
fn apply_lwjgl3ify_patches(
    minecraft_dir: &Path,
    classpath: &mut Vec<PathBuf>,
) -> (Vec<String>, Option<String>) {
    let mods_dir = minecraft_dir.join("mods");
    let lwjgl3ify_jar = match find_lwjgl3ify_jar(&mods_dir) {
        Some(p) => p,
        None => return (Vec::new(), None),
    };

    let patches_dest = minecraft_dir.join(".forge-patches.zip");

    if let Err(e) = extract_forge_patches(&lwjgl3ify_jar, &patches_dest) {
        tracing::warn!("Failed to extract lwjgl3ify forge patches: {e}");
        return (Vec::new(), None);
    }

    // prepend so patched classes shadow vanilla launchwrapper
    classpath.insert(0, patches_dest.clone());

    let mut jvm_args = parse_add_opens(&patches_dest).unwrap_or_default();

    // RFB requires its own classloader to be the system classloader, and
    // its Main class handles bootstrapping into launchwrapper.
    // the other flags match lwjgl3ify's java9args.txt.
    jvm_args.extend([
        "-Djava.system.class.loader=com.gtnewhorizons.retrofuturabootstrap.RfbSystemClassLoader".to_string(),
        "-Dfile.encoding=UTF-8".to_string(),
        "--enable-native-access".to_string(),
        "ALL-UNNAMED".to_string(),
    ]);

    // also prepend the lwjgl3ify jar itself so RFB can find its plugin classes
    classpath.insert(1, lwjgl3ify_jar);

    let main_class = "com.gtnewhorizons.retrofuturabootstrap.Main".to_string();

    (jvm_args, Some(main_class))
}

fn find_lwjgl3ify_jar(mods_dir: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(mods_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("lwjgl3ify") && name.ends_with(".jar") {
            return Some(entry.path());
        }
    }
    None
}

fn extract_forge_patches(lwjgl3ify_jar: &Path, dest: &Path) -> Result<(), std::io::Error> {
    use std::io::Read;

    let file = std::fs::File::open(lwjgl3ify_jar)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut entry = archive
        .by_name("me/eigenraven/lwjgl3ify/relauncher/forgePatches.zip")
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e))?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf)?;
    std::fs::write(dest, &buf)
}

fn parse_add_opens(patches_zip: &Path) -> Option<Vec<String>> {
    use std::io::Read;

    let file = std::fs::File::open(patches_zip).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name("META-INF/MANIFEST.MF").ok()?;
    let mut manifest = String::new();
    entry.read_to_string(&mut manifest).ok()?;

    // manifest continuation lines start with a single space
    let manifest = manifest.replace("\r\n ", "").replace("\n ", "");

    let mut args = Vec::new();
    for line in manifest.lines() {
        if let Some(value) = line.strip_prefix("Add-Opens: ") {
            for module_package in value.split_whitespace() {
                args.push("--add-opens".to_string());
                args.push(format!("{module_package}=ALL-UNNAMED"));
            }
        }
    }

    Some(args)
}
