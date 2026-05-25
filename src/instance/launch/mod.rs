// builds the full java command line and spawns minecraft as a child process.
// handles classpath assembly, auth token injection, and log capture.
// loader-specific patches live in submodules (e.g. patches.rs for lwjgl3ify).

mod patches;

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::auth::AccountType;
use crate::instance::models::{InstanceConfig, ModLoader};
use crate::launch_profile::model::LaunchProfile;
use crate::launch_profile::rules::{self, FeatureSet, RuleContext};
use crate::launch_profile::templates::TemplateContext;
use crate::launch_profile::{render, resolve, system};

#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("Version metadata not found: {0}. Re-create the instance to fix this.")]
    MetaNotFound(String),
    #[error("Profile error: {0}")]
    Parse(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0} launch is not yet supported")]
    NotSupported(String),
    #[error("{0}")]
    Auth(String),
}

fn build_game_args(
    profile: &LaunchProfile,
    rule_ctx: &RuleContext<'_>,
    template_ctx: &TemplateContext<'_>,
) -> Result<(Vec<String>, Vec<String>), LaunchError> {
    let rendered = render::render_args(profile, rule_ctx, template_ctx)
        .map_err(|e| LaunchError::Parse(format!("Failed to render args: {e}")))?;
    Ok((rendered.jvm, rendered.game))
}

// existing installs from rmcl <= 0.3.0 have meta.json files in the
// stripped legacy format (no `arguments`, no `minecraftArguments`). every
// real upstream profile has at least one of those fields. on detecting the
// stripped format, re-fetch the version metadata from mojang's manifest
// and overwrite the file with the raw upstream bytes.
async fn migrate_legacy_meta_if_needed(
    meta_path: &Path,
    profile: &LaunchProfile,
    game_version: &str,
) -> Result<Option<LaunchProfile>, LaunchError> {
    if profile.arguments.is_some() || profile.minecraft_arguments.is_some() {
        return Ok(None);
    }

    tracing::warn!(
        "Cached meta.json for {game_version} is missing arguments; re-fetching from Mojang"
    );

    let client = crate::net::HttpClient::new();
    let manifest = match crate::net::mojang::fetch_version_manifest(&client).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                "Could not reach Mojang manifest ({e}); proceeding with the cached legacy profile. \
                 Modern features like Forge's --add-opens flags may be missing until the next online launch."
            );
            return Ok(None);
        }
    };

    let entry = manifest
        .versions
        .iter()
        .find(|v| v.id == game_version)
        .ok_or_else(|| {
            LaunchError::Parse(format!(
                "Version {game_version} not found in Mojang manifest"
            ))
        })?;

    let (_meta, raw) = match crate::net::mojang::fetch_version_meta_with_raw(&client, entry).await {
        Ok(ok) => ok,
        Err(e) => {
            tracing::warn!(
                "Could not refetch version metadata from Mojang ({e}); proceeding with the cached legacy profile."
            );
            return Ok(None);
        }
    };

    tokio::fs::write(meta_path, &raw).await?;

    let refreshed: LaunchProfile = serde_json::from_slice(&raw)
        .map_err(|e| LaunchError::Parse(format!("Failed to parse refreshed meta: {e}")))?;
    Ok(Some(refreshed))
}

// the forge/neoforge installer writes its version JSON to a path that's
// loader-specific. encode the naming convention here so migration code
// can find the original file when it needs to rebuild our cache.
fn installer_version_dir_name(
    loader: ModLoader,
    game_version: &str,
    loader_version: &str,
) -> Option<String> {
    match loader {
        ModLoader::Forge => Some(format!("{game_version}-forge-{loader_version}")),
        ModLoader::NeoForge => Some(format!("neoforge-{loader_version}")),
        ModLoader::Vanilla | ModLoader::Fabric | ModLoader::Quilt => None,
    }
}

// loader profiles installed by rmcl <= 0.3.0 are in our stripped
// `{mainClass, libraries[, gameArguments]}` format, which silently drops
// `inheritsFrom`, `arguments.jvm`, and conditional rules from upstream.
// detect that shape (no inheritsFrom AND no arguments AND no
// minecraftArguments - every real upstream profile has at least one) and
// rebuild from the installer's original JSON if it's still on disk.
async fn migrate_legacy_loader_profile_if_needed(
    profile_path: &Path,
    profile: &LaunchProfile,
    config: &InstanceConfig,
    instance_dir: &Path,
) -> Result<Option<LaunchProfile>, LaunchError> {
    // Fabric and Quilt fetch their profiles from a network endpoint at
    // install time; there's no installer-written JSON on disk to recover
    // from. their upstream profiles also happen to match the "legacy
    // stripped" predicate (no inheritsFrom, no arguments), so without this
    // early return every Fabric/Quilt launch would incorrectly fail
    // migration. resolve() handles their lack of inheritsFrom via the
    // implicit fallback in the launch flow.
    if matches!(config.loader, ModLoader::Fabric | ModLoader::Quilt) {
        return Ok(None);
    }

    // tightened predicate per the spec: only treat a profile as "legacy
    // stripped" when our old `gameArguments` field is present. that field
    // is unique to rmcl <= 0.3.0's custom shape; no upstream profile
    // emits it. without this gate, an upstream profile that happens to
    // omit inheritsFrom/arguments/minecraftArguments would be mistakenly
    // re-extracted from the installer JSON.
    let is_legacy = profile.inherits_from.is_none()
        && profile.arguments.is_none()
        && profile.minecraft_arguments.is_none()
        && profile.game_arguments.is_some();
    if !is_legacy {
        return Ok(None);
    }

    let Some(loader_version) = config.loader_version.as_deref() else {
        return Err(LaunchError::Parse(format!(
            "Loader profile at {} is in an outdated format and the instance config has no \
             loader_version recorded. Reinstall {} for this instance.",
            profile_path.display(),
            config.loader
        )));
    };
    let Some(version_dir) =
        installer_version_dir_name(config.loader, &config.game_version, loader_version)
    else {
        // unreachable today: only Vanilla/Fabric/Quilt return None, and
        // Vanilla doesn't pass this code path (no loader profile to
        // migrate) while Fabric/Quilt are filtered above.
        return Err(LaunchError::Parse(format!(
            "Loader profile at {} is in an outdated format. Reinstall {} for this instance.",
            profile_path.display(),
            config.loader
        )));
    };

    let installer_json_path = instance_dir
        .join(".minecraft")
        .join("versions")
        .join(&version_dir)
        .join(format!("{version_dir}.json"));

    if !installer_json_path.exists() {
        return Err(LaunchError::Parse(format!(
            "Loader profile at {} is in an outdated format and the installer JSON at {} \
             is missing. Reinstall {} for this instance.",
            profile_path.display(),
            installer_json_path.display(),
            config.loader
        )));
    }

    tracing::warn!(
        "Loader profile {} is in legacy format; rebuilding from {}",
        profile_path.display(),
        installer_json_path.display()
    );

    let raw = tokio::fs::read(&installer_json_path).await?;
    tokio::fs::write(profile_path, &raw).await?;

    let refreshed: LaunchProfile = serde_json::from_slice(&raw).map_err(|e| {
        LaunchError::Parse(format!("Failed to parse refreshed loader profile: {e}"))
    })?;
    Ok(Some(refreshed))
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
    let meta: LaunchProfile = serde_json::from_slice(&tokio::fs::read(&meta_path).await?)?;
    let meta = match migrate_legacy_meta_if_needed(&meta_path, &meta, &config.game_version).await? {
        Some(refreshed) => refreshed,
        None => meta,
    };

    let current_features = FeatureSet::default();
    let host_os_version = system::mojang_os_version();
    let rule_ctx = RuleContext {
        os_name: system::mojang_os_name(),
        os_version: &host_os_version,
        arch: system::mojang_arch_name(),
        features: &current_features,
    };

    let asset_index_id = meta
        .asset_index
        .as_ref()
        .map(|ai| ai.id.clone())
        .unwrap_or_default();

    let lib_dir = meta_dir.join("libraries");

    let lv = config.loader_version.as_deref().unwrap_or("unknown");
    let profile_filename = match config.loader {
        ModLoader::Vanilla => None,
        ModLoader::Fabric => Some(format!("fabric-{}-{}.json", config.game_version, lv)),
        ModLoader::Quilt => Some(format!("quilt-{}-{}.json", config.game_version, lv)),
        ModLoader::Forge => Some(format!("forge-{}-{}.json", config.game_version, lv)),
        ModLoader::NeoForge => Some(format!("neoforge-{}.json", lv)),
    };

    // load the loader profile (if any), migrate from the old stripped format
    // if needed, and resolve `inheritsFrom` against the vanilla parent (which
    // the vanilla meta migration above ensured is fresh on disk). when no
    // loader is configured we use the already-loaded vanilla meta directly.
    let merged_profile: LaunchProfile = if let Some(filename) = &profile_filename {
        let profile_path = meta_dir.join("loader-profiles").join(filename);
        if !profile_path.exists() {
            return Err(LaunchError::MetaNotFound(
                profile_path.display().to_string(),
            ));
        }
        let mut loader_profile: LaunchProfile =
            serde_json::from_slice(&tokio::fs::read(&profile_path).await?)?;

        if let Some(refreshed) = migrate_legacy_loader_profile_if_needed(
            &profile_path,
            &loader_profile,
            config,
            &instance_dir,
        )
        .await?
        {
            loader_profile = refreshed;
        }

        // legacy installer-written profiles (and any loader profile that
        // omits inheritsFrom) still need to be layered over vanilla. set
        // the inherit explicitly so resolve() walks the chain.
        if loader_profile.inherits_from.is_none() {
            loader_profile.inherits_from = Some(config.game_version.clone());
        }

        resolve::resolve(loader_profile, meta_dir)
            .await
            .map_err(|e| LaunchError::Parse(format!("Failed to resolve loader profile: {e}")))?
    } else {
        meta.clone()
    };

    let main_class = merged_profile
        .main_class
        .clone()
        .ok_or_else(|| LaunchError::Parse("merged profile missing mainClass".into()))?;

    // rebuild the classpath from the merged profile. vanilla-style libraries
    // have `downloads.artifact.path` set and live in meta_dir/libraries/.
    // loader-style libraries only have a maven coordinate; for forge/neoforge,
    // the installer drops some of them into <instance>/.minecraft/libraries/
    // so we check there first.
    let has_local_libs = matches!(config.loader, ModLoader::Forge | ModLoader::NeoForge);
    let local_lib_dir = minecraft_dir.join("libraries");

    let mut classpath: Vec<PathBuf> = Vec::new();
    for lib in &merged_profile.libraries {
        if let Some(rules) = &lib.rules
            && !rules::evaluate(rules, &rule_ctx)
        {
            continue;
        }

        // resolve a relative path for this library. prefer downloads.artifact.path
        // when present (vanilla-style), fall back to maven_coord_to_path(name)
        // for loader-style entries that only have a coord.
        let rel: PathBuf = match lib
            .downloads
            .as_ref()
            .and_then(|d| d.artifact.as_ref())
            .map(|a| PathBuf::from(&a.path))
            .or_else(|| crate::net::maven_coord_to_path(&lib.name).map(PathBuf::from))
        {
            Some(p) => p,
            None => continue,
        };

        // for forge/neoforge, the installer drops some libs (notably the
        // bootstrap library) into <instance>/.minecraft/libraries/ rather
        // than the shared meta cache. check there first regardless of
        // whether the lib has a downloads.artifact entry.
        if has_local_libs {
            let in_local = local_lib_dir.join(&rel);
            if in_local.exists() {
                classpath.push(in_local);
                continue;
            }
        }
        classpath.push(lib_dir.join(rel));
    }

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

    // resolve auth credentials, refreshing the microsoft token if needed.
    let mut account_store = crate::auth::AccountStore::load();
    let (mc_username, mc_uuid, mc_token, mc_user_type) = match account_store
        .active_account()
        .cloned()
    {
        Some(acc) => {
            // offline accounts can only launch if a microsoft account exists
            // (proves the user owns minecraft).
            if acc.account_type != AccountType::Microsoft && !account_store.has_microsoft_account()
            {
                return Err(LaunchError::Auth(
                    "Offline accounts require a Microsoft account that owns Minecraft".to_owned(),
                ));
            }
            let (token, new_refresh, new_expires) = match acc.account_type {
                AccountType::Microsoft => match crate::auth::refresh_and_get_token(&acc).await {
                    Ok(triple) => triple,
                    Err(e) => {
                        return Err(LaunchError::Auth(format!("Authentication failed: {e}")));
                    }
                },
                AccountType::Offline => ("0".to_string(), None, None),
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
                AccountType::Microsoft => "msa",
                AccountType::Offline => "legacy",
            };
            (
                acc.username.clone(),
                acc.uuid.clone(),
                token,
                user_type.to_string(),
            )
        }
        None => return Err(LaunchError::Auth("No account selected".to_owned())),
    };

    let assets_root = meta_dir.join("assets");
    let natives_dir = meta_dir
        .join("versions")
        .join(&config.game_version)
        .join("natives");
    let version_type = merged_profile.type_.as_deref().unwrap_or("release");
    let template_ctx = TemplateContext {
        library_directory: &lib_dir,
        classpath_separator: sep,
        version_name: &config.game_version,
        version_type,
        natives_directory: &natives_dir,
        classpath: &cp_str,
        game_directory: &minecraft_dir,
        assets_root: &assets_root,
        assets_index_name: &asset_index_id,
        auth_player_name: &mc_username,
        auth_uuid: &mc_uuid,
        auth_access_token: &mc_token,
        auth_xuid: "0",
        user_type: &mc_user_type,
        user_properties: "{}",
        launcher_name: "rmcl",
        launcher_version: env!("CARGO_PKG_VERSION"),
        clientid: "0",
        resolution_width: "",
        resolution_height: "",
    };

    let (upstream_jvm_args, game_args) =
        build_game_args(&merged_profile, &rule_ctx, &template_ctx)?;

    let mut jvm: Vec<String> = vec![
        format!("-Xms{}", config.memory_min.as_deref().unwrap_or("512M")),
        format!("-Xmx{}", config.memory_max.as_deref().unwrap_or("2G")),
    ];
    jvm.extend(patch_jvm_args);
    jvm.extend(upstream_jvm_args);
    jvm.extend(config.jvm_args.clone());

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

    #[test]
    fn build_game_args_renders_upstream_arguments_and_appends_loader_args() {
        use crate::launch_profile::model::{Argument, Arguments, LaunchProfile};
        use crate::launch_profile::rules::{FeatureSet, RuleContext};
        use TemplateContext;
        use std::path::PathBuf;

        let lib = PathBuf::from("/m/libraries");
        let nat = PathBuf::from("/m/natives");
        let game_dir = PathBuf::from("/i/.minecraft");
        let assets = PathBuf::from("/m/assets");

        let template_ctx = TemplateContext {
            library_directory: &lib,
            classpath_separator: ":",
            version_name: "1.20.1",
            natives_directory: &nat,
            classpath: "a.jar:b.jar",
            game_directory: &game_dir,
            assets_root: &assets,
            assets_index_name: "5",
            auth_player_name: "Player",
            auth_uuid: "00000000-0000-0000-0000-000000000000",
            auth_access_token: "token",
            auth_xuid: "0",
            user_type: "msa",
            user_properties: "{}",
            launcher_name: "rmcl",
            launcher_version: "test",
            clientid: "0",
            version_type: "release",
            resolution_width: "",
            resolution_height: "",
        };
        let features = FeatureSet::default();
        let rule_ctx = RuleContext {
            os_name: "linux",
            os_version: "6.0",
            arch: "x86_64",
            features: &features,
        };

        let profile = LaunchProfile {
            id: "1.20.1".into(),
            inherits_from: None,
            main_class: Some("net.minecraft.client.main.Main".into()),
            libraries: Vec::new(),
            arguments: Some(Arguments {
                game: vec![
                    Argument::Literal("--username".into()),
                    Argument::Literal("${auth_player_name}".into()),
                ],
                jvm: vec![Argument::Literal(
                    "-Djava.library.path=${natives_directory}".into(),
                )],
            }),
            ..Default::default()
        };

        let (jvm, game_args) = build_game_args(&profile, &rule_ctx, &template_ctx).unwrap();
        assert_eq!(jvm, vec!["-Djava.library.path=/m/natives"]);
        assert_eq!(game_args, vec!["--username", "Player"]);
    }

    #[test]
    fn migration_predicate_triggers_when_both_argument_fields_absent() {
        use LaunchProfile;
        let stripped = LaunchProfile {
            id: "1.20.1".into(),
            inherits_from: None,
            main_class: Some("net.test.Main".into()),
            libraries: Vec::new(),
            ..Default::default()
        };
        assert!(stripped.arguments.is_none() && stripped.minecraft_arguments.is_none());
    }

    #[test]
    fn migration_predicate_skips_when_modern_arguments_present() {
        use crate::launch_profile::model::{Arguments, LaunchProfile};
        let modern = LaunchProfile {
            id: "1.20.1".into(),
            inherits_from: None,
            main_class: Some("net.test.Main".into()),
            libraries: Vec::new(),
            arguments: Some(Arguments::default()),
            ..Default::default()
        };
        assert!(modern.arguments.is_some());
    }

    #[test]
    fn migration_predicate_skips_when_legacy_minecraft_arguments_present() {
        use LaunchProfile;
        let legacy = LaunchProfile {
            id: "1.7.10".into(),
            inherits_from: None,
            main_class: Some("net.test.Main".into()),
            libraries: Vec::new(),
            arguments: None,
            minecraft_arguments: Some("--username Player".into()),
            ..Default::default()
        };
        assert!(legacy.minecraft_arguments.is_some());
    }

    #[test]
    fn installer_version_dir_name_for_forge() {
        assert_eq!(
            installer_version_dir_name(ModLoader::Forge, "1.20.1", "47.2.0"),
            Some("1.20.1-forge-47.2.0".to_string())
        );
    }

    #[test]
    fn installer_version_dir_name_for_neoforge() {
        assert_eq!(
            installer_version_dir_name(ModLoader::NeoForge, "1.21.1", "21.1.0"),
            Some("neoforge-21.1.0".to_string())
        );
    }

    #[test]
    fn installer_version_dir_name_for_non_installer_loaders() {
        assert!(installer_version_dir_name(ModLoader::Vanilla, "1.20.1", "v").is_none());
        assert!(installer_version_dir_name(ModLoader::Fabric, "1.20.1", "0.14.21").is_none());
        assert!(installer_version_dir_name(ModLoader::Quilt, "1.20.1", "0.20.0").is_none());
    }

    #[test]
    fn loader_profile_legacy_predicate_triggers_when_all_three_absent() {
        use LaunchProfile;
        let legacy = LaunchProfile {
            id: "1.20.1-forge-47.2.0".into(),
            inherits_from: None,
            main_class: Some("cpw.mods.bootstraplauncher.BootstrapLauncher".into()),
            libraries: Vec::new(),
            ..Default::default()
        };
        let is_legacy = legacy.inherits_from.is_none()
            && legacy.arguments.is_none()
            && legacy.minecraft_arguments.is_none();
        assert!(is_legacy);
    }

    #[test]
    fn loader_profile_legacy_predicate_skips_modern_profile_with_inherits_from() {
        use LaunchProfile;
        let modern = LaunchProfile {
            id: "1.20.1-forge-47.2.0".into(),
            inherits_from: Some("1.20.1".into()),
            main_class: Some("cpw.mods.bootstraplauncher.BootstrapLauncher".into()),
            libraries: Vec::new(),
            ..Default::default()
        };
        let is_legacy = modern.inherits_from.is_none()
            && modern.arguments.is_none()
            && modern.minecraft_arguments.is_none();
        assert!(!is_legacy);
    }

    #[tokio::test]
    async fn migrate_legacy_loader_profile_skips_fabric() {
        // a fresh upstream Fabric profile happens to match the "legacy"
        // shape (no inheritsFrom, no arguments, no minecraftArguments).
        // make sure the migration helper recognises this is Fabric and
        // returns Ok(None) instead of erroring with "reinstall Fabric".
        use LaunchProfile;
        use chrono::Utc;
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let instance_dir = tmp.path().join("instance");
        std::fs::create_dir_all(&instance_dir).unwrap();
        let profile_path = tmp.path().join("fabric-1.20.1-0.14.21.json");
        std::fs::write(&profile_path, b"{}").unwrap();

        let upstream_fabric_shape = LaunchProfile {
            id: "fabric-loader-0.14.21-1.20.1".into(),
            inherits_from: None,
            main_class: Some("net.fabricmc.loader.impl.launch.knot.KnotClient".into()),
            libraries: Vec::new(),
            ..Default::default()
        };

        let config = InstanceConfig {
            name: "test".into(),
            game_version: "1.20.1".into(),
            loader: ModLoader::Fabric,
            loader_version: Some("0.14.21".into()),
            created: Utc::now(),
            last_played: None,
            java_path: None,
            memory_max: None,
            memory_min: None,
            jvm_args: Vec::new(),
            resolution: None,
        };

        let result = migrate_legacy_loader_profile_if_needed(
            &profile_path,
            &upstream_fabric_shape,
            &config,
            &instance_dir,
        )
        .await;

        assert!(
            matches!(result, Ok(None)),
            "expected Ok(None), got {result:?}"
        );
    }
}
