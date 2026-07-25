use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigSyncError {
    #[error("Invalid config sync profile: {0}")]
    InvalidProfile(String),
    #[error("Cannot switch config profiles while '{instance}' is running")]
    InstanceRunning { instance: String },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug)]
pub struct ConfigSyncLock {
    file: std::fs::File,
}

impl Drop for ConfigSyncLock {
    fn drop(&mut self) {
        if let Err(e) = self.file.unlock() {
            tracing::warn!("Failed to release config sync lock: {}", e);
        }
    }
}

pub fn prepare(
    profile: Option<&str>,
    meta_dir: &Path,
    minecraft_dir: &Path,
) -> Result<bool, ConfigSyncError> {
    let Some(profile) = profile.and_then(normalize_profile) else {
        return Ok(false);
    };
    validate_profile(profile)?;

    let profile_dir = profile_dir(meta_dir, profile);
    if !profile_dir.exists() {
        return Ok(false);
    }
    let _lock = acquire_lock(&profile_dir)?;

    if !profile_payload_exists(&profile_dir)? {
        sync_to_profile(minecraft_dir, &profile_dir)?;
    } else {
        sync_from_profile(&profile_dir, minecraft_dir)?;
    }

    Ok(true)
}

pub fn finish(
    profile: Option<&str>,
    meta_dir: &Path,
    minecraft_dir: &Path,
) -> Result<(), ConfigSyncError> {
    let Some(profile) = profile.and_then(normalize_profile) else {
        return Ok(());
    };
    validate_profile(profile)?;

    let profile_dir = profile_dir(meta_dir, profile);
    let _lock = acquire_lock(&profile_dir)?;
    sync_to_profile(minecraft_dir, &profile_dir)
}

pub fn list_profiles(meta_dir: &Path) -> Result<Vec<String>, ConfigSyncError> {
    let root = profiles_dir(meta_dir);
    let mut profiles = Vec::new();
    if !root.exists() {
        return Ok(profiles);
    }

    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if validate_profile(&name).is_ok() {
                profiles.push(name);
            }
        }
    }
    profiles.sort_unstable();
    Ok(profiles)
}

pub fn create_profile(meta_dir: &Path, profile: &str) -> Result<String, ConfigSyncError> {
    let Some(profile) = normalize_profile(profile) else {
        return Err(ConfigSyncError::InvalidProfile(profile.to_string()));
    };
    validate_profile(profile)?;
    std::fs::create_dir_all(profile_dir(meta_dir, profile))?;
    Ok(profile.to_string())
}

pub fn delete_profile(meta_dir: &Path, profile: &str) -> Result<(), ConfigSyncError> {
    validate_profile(profile)?;
    let dir = profile_dir(meta_dir, profile);
    if dir.exists() {
        remove_path(&dir)?;
    }
    Ok(())
}

pub fn switch_profile(
    instance_name: &str,
    current_profile: Option<&str>,
    target_profile: Option<&str>,
    meta_dir: &Path,
    instance_dir: &Path,
) -> Result<Option<String>, ConfigSyncError> {
    if crate::running::get(instance_name).is_some() {
        return Err(ConfigSyncError::InstanceRunning {
            instance: instance_name.to_string(),
        });
    }

    let current_profile = current_profile.and_then(normalize_profile);
    let target_profile = target_profile.and_then(normalize_profile);
    if let Some(profile) = current_profile {
        validate_profile(profile)?;
    }
    if let Some(profile) = target_profile {
        validate_profile(profile)?;
    }

    if current_profile == target_profile {
        return Ok(current_profile.map(str::to_string));
    }

    if let Some(profile) = current_profile {
        let profile_dir = profile_dir(meta_dir, profile);
        if profile_dir.exists() {
            let _lock = acquire_lock(&profile_dir)?;
            sync_to_profile(&minecraft_dir(instance_dir), &profile_dir)?;
        }
    }

    match (current_profile, target_profile) {
        (None, Some(_)) => {
            sync_to_profile(
                &minecraft_dir(instance_dir),
                &local_backup_dir(instance_dir),
            )?;
        }
        (Some(_), None) => {
            let backup = local_backup_dir(instance_dir);
            if backup.exists() {
                sync_from_profile(&backup, &minecraft_dir(instance_dir))?;
            }
            return Ok(None);
        }
        _ => {}
    }

    let Some(profile) = target_profile else {
        return Ok(None);
    };

    let profile_dir = profile_dir(meta_dir, profile);
    let _lock = acquire_lock(&profile_dir)?;

    if !profile_payload_exists(&profile_dir)? {
        sync_to_profile(&minecraft_dir(instance_dir), &profile_dir)?;
    }
    sync_from_profile(&profile_dir, &minecraft_dir(instance_dir))?;

    Ok(Some(profile.to_string()))
}

fn normalize_profile(profile: &str) -> Option<&str> {
    let trimmed = profile.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

pub fn validate_profile(profile: &str) -> Result<(), ConfigSyncError> {
    if profile.is_empty()
        || profile.len() > 64
        || profile.starts_with('.')
        || profile.contains('/')
        || profile.contains('\\')
        || profile.eq_ignore_ascii_case("default")
        || profile.eq_ignore_ascii_case("none")
        || profile.eq_ignore_ascii_case("local")
        || profile.eq_ignore_ascii_case("instance default")
        || profile.eq_ignore_ascii_case("local default")
        || profile
            .chars()
            .any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return Err(ConfigSyncError::InvalidProfile(profile.to_string()));
    }
    Ok(())
}

fn profile_dir(meta_dir: &Path, profile: &str) -> PathBuf {
    profiles_dir(meta_dir).join(profile)
}

fn profiles_dir(meta_dir: &Path) -> PathBuf {
    crate::storage::MetadataPaths::new(meta_dir).profiles()
}

fn minecraft_dir(instance_dir: &Path) -> PathBuf {
    instance_dir.join(crate::storage::MINECRAFT_DIR_NAME)
}

fn local_backup_dir(instance_dir: &Path) -> PathBuf {
    crate::storage::InstancePaths::new(instance_dir).local_config()
}

fn acquire_lock(profile_dir: &Path) -> Result<ConfigSyncLock, ConfigSyncError> {
    std::fs::create_dir_all(profile_dir)?;
    let path = profile_dir.join(".lock");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock()?;
    Ok(ConfigSyncLock { file })
}

fn mirror_dir(src: &Path, dst: &Path) -> Result<(), ConfigSyncError> {
    if dst.exists() {
        remove_path(dst)?;
    }
    std::fs::create_dir_all(dst)?;

    if !src.exists() {
        return Ok(());
    }

    copy_dir_contents(src, dst)
}

fn sync_to_profile(minecraft_dir: &Path, profile_dir: &Path) -> Result<(), ConfigSyncError> {
    mirror_dir(&minecraft_dir.join("config"), &profile_dir.join("config"))?;
    mirror_options(minecraft_dir, profile_dir)
}

fn sync_from_profile(profile_dir: &Path, minecraft_dir: &Path) -> Result<(), ConfigSyncError> {
    mirror_dir(&profile_dir.join("config"), &minecraft_dir.join("config"))?;
    mirror_options(profile_dir, minecraft_dir)
}

fn profile_payload_exists(profile_dir: &Path) -> Result<bool, ConfigSyncError> {
    if profile_dir.join("config").exists() {
        return Ok(true);
    }
    if !profile_dir.exists() {
        return Ok(false);
    }
    for entry in std::fs::read_dir(profile_dir)? {
        let entry = entry?;
        if entry.file_type()?.is_file() && is_options_file(&entry.file_name().to_string_lossy()) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn mirror_options(src: &Path, dst: &Path) -> Result<(), ConfigSyncError> {
    remove_options(dst)?;
    std::fs::create_dir_all(dst)?;
    if !src.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if entry.file_type()?.is_file() && is_options_file(&name.to_string_lossy()) {
            std::fs::copy(entry.path(), dst.join(name))?;
        }
    }
    Ok(())
}

fn remove_options(dir: &Path) -> Result<(), ConfigSyncError> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if is_options_file(&entry.file_name().to_string_lossy()) {
            remove_path(&entry.path())?;
        }
    }
    Ok(())
}

fn is_options_file(name: &str) -> bool {
    name == "options.txt" || name.starts_with("options") && name.ends_with(".txt")
}

fn remove_path(path: &Path) -> Result<(), ConfigSyncError> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)?;
    } else {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn copy_dir_contents(src: &Path, dst: &Path) -> Result<(), ConfigSyncError> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let source = entry.path();
        let target = dst.join(entry.file_name());
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_dir_contents(&source, &target)?;
        } else if file_type.is_file() {
            std::fs::copy(&source, &target)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_prepare_seeds_shared_config() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = tmp.path().join("meta");
        let minecraft = tmp.path().join("instance/minecraft");
        create_profile(&meta, "main").unwrap();
        std::fs::create_dir_all(minecraft.join("config/nested")).unwrap();
        std::fs::write(minecraft.join("options.txt"), "local-options").unwrap();
        std::fs::write(minecraft.join("optionsshaders.txt"), "shader-options").unwrap();
        std::fs::write(minecraft.join("config/options.txt"), "local-config").unwrap();
        std::fs::write(minecraft.join("config/nested/mod.toml"), "nested").unwrap();

        assert!(prepare(Some("main"), &meta, &minecraft).unwrap());

        assert_eq!(
            std::fs::read_to_string(meta.join("state/profiles/main/options.txt")).unwrap(),
            "local-options"
        );
        assert_eq!(
            std::fs::read_to_string(meta.join("state/profiles/main/optionsshaders.txt")).unwrap(),
            "shader-options"
        );
        assert_eq!(
            std::fs::read_to_string(meta.join("state/profiles/main/config/options.txt")).unwrap(),
            "local-config"
        );
        assert_eq!(
            std::fs::read_to_string(meta.join("state/profiles/main/config/nested/mod.toml"))
                .unwrap(),
            "nested"
        );
    }

    #[test]
    fn prepare_mirrors_shared_config_into_instance() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = tmp.path().join("meta");
        let minecraft = tmp.path().join("instance/minecraft");
        std::fs::create_dir_all(meta.join("state/profiles/main/config")).unwrap();
        std::fs::create_dir_all(minecraft.join("config")).unwrap();
        std::fs::write(
            meta.join("state/profiles/main/options.txt"),
            "shared-options",
        )
        .unwrap();
        std::fs::write(
            meta.join("state/profiles/main/config/shared.toml"),
            "shared",
        )
        .unwrap();
        std::fs::write(minecraft.join("options.txt"), "stale-options").unwrap();
        std::fs::write(minecraft.join("config/local.toml"), "stale").unwrap();

        assert!(prepare(Some("main"), &meta, &minecraft).unwrap());

        assert_eq!(
            std::fs::read_to_string(minecraft.join("options.txt")).unwrap(),
            "shared-options"
        );
        assert_eq!(
            std::fs::read_to_string(minecraft.join("config/shared.toml")).unwrap(),
            "shared"
        );
        assert!(!minecraft.join("config/local.toml").exists());
    }

    #[test]
    fn finish_mirrors_instance_config_back_to_shared() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = tmp.path().join("meta");
        let minecraft = tmp.path().join("instance/minecraft");
        std::fs::create_dir_all(meta.join("state/profiles/main/config")).unwrap();
        std::fs::write(meta.join("state/profiles/main/config/old.toml"), "old").unwrap();
        std::fs::create_dir_all(minecraft.join("config")).unwrap();
        std::fs::write(minecraft.join("options.txt"), "new-options").unwrap();
        std::fs::write(minecraft.join("config/new.toml"), "new").unwrap();

        finish(Some("main"), &meta, &minecraft).unwrap();

        assert_eq!(
            std::fs::read_to_string(meta.join("state/profiles/main/options.txt")).unwrap(),
            "new-options"
        );
        assert_eq!(
            std::fs::read_to_string(meta.join("state/profiles/main/config/new.toml")).unwrap(),
            "new"
        );
        assert!(!meta.join("state/profiles/main/config/old.toml").exists());
    }

    #[test]
    fn prepare_releases_lock_for_another_instance() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = tmp.path().join("meta");
        let minecraft = tmp.path().join("instance/minecraft");
        create_profile(&meta, "main").unwrap();
        std::fs::create_dir_all(minecraft.join("config")).unwrap();

        assert!(prepare(Some("main"), &meta, &minecraft).unwrap());
        let second = tmp.path().join("second/minecraft");
        std::fs::create_dir_all(second.join("config")).unwrap();

        assert!(prepare(Some("main"), &meta, &second).unwrap());
    }

    #[test]
    fn profile_rejects_path_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        let err = prepare(Some("../bad"), tmp.path(), tmp.path()).unwrap_err();

        assert!(matches!(err, ConfigSyncError::InvalidProfile(_)));
    }

    #[test]
    fn profile_rejects_builtin_names() {
        for profile in [
            "none",
            "default",
            "local",
            "instance default",
            "local default",
        ] {
            let err = validate_profile(profile).unwrap_err();
            assert!(matches!(err, ConfigSyncError::InvalidProfile(_)));
        }
    }

    #[test]
    fn prepare_ignores_deleted_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = tmp.path().join("meta");
        let minecraft = tmp.path().join("instance/minecraft");
        std::fs::create_dir_all(minecraft.join("config")).unwrap();

        let lock = prepare(Some("deleted"), &meta, &minecraft).unwrap();

        assert!(!lock);
        assert!(!meta.join("state/profiles/deleted").exists());
    }

    #[test]
    fn create_profile_trims_and_lists_profiles() {
        let tmp = tempfile::tempdir().unwrap();

        let profile = create_profile(tmp.path(), " main ").unwrap();
        let profiles = list_profiles(tmp.path()).unwrap();

        assert_eq!(profile, "main");
        assert_eq!(profiles, vec!["main"]);
    }

    #[test]
    fn delete_profile_removes_profile_dir() {
        let tmp = tempfile::tempdir().unwrap();
        create_profile(tmp.path(), "main").unwrap();

        delete_profile(tmp.path(), "main").unwrap();

        assert!(list_profiles(tmp.path()).unwrap().is_empty());
    }

    #[test]
    fn switch_to_profile_backs_up_local_config_and_restores_none() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = tmp.path().join("meta");
        let instance = tmp.path().join("instance");
        let minecraft = instance.join(crate::storage::MINECRAFT_DIR_NAME);
        std::fs::create_dir_all(minecraft.join("config")).unwrap();
        std::fs::write(minecraft.join("options.txt"), "local-options").unwrap();
        std::fs::write(minecraft.join("config/local.txt"), "local").unwrap();

        let selected = switch_profile("inst", None, Some("main"), &meta, &instance).unwrap();
        assert_eq!(selected.as_deref(), Some("main"));
        assert_eq!(
            std::fs::read_to_string(instance.join("rmcl/content/config/options.txt")).unwrap(),
            "local-options"
        );
        assert_eq!(
            std::fs::read_to_string(instance.join("rmcl/content/config/config/local.txt")).unwrap(),
            "local"
        );

        std::fs::write(minecraft.join("options.txt"), "shared-options").unwrap();
        std::fs::write(minecraft.join("config/shared.txt"), "shared").unwrap();
        let selected = switch_profile("inst", Some("main"), None, &meta, &instance).unwrap();

        assert_eq!(selected, None);
        assert_eq!(
            std::fs::read_to_string(meta.join("state/profiles/main/options.txt")).unwrap(),
            "shared-options"
        );
        assert_eq!(
            std::fs::read_to_string(meta.join("state/profiles/main/config/shared.txt")).unwrap(),
            "shared"
        );
        assert_eq!(
            std::fs::read_to_string(minecraft.join("options.txt")).unwrap(),
            "local-options"
        );
        assert_eq!(
            std::fs::read_to_string(minecraft.join("config/local.txt")).unwrap(),
            "local"
        );
        assert!(!minecraft.join("config/shared.txt").exists());
    }

    #[test]
    fn switch_from_deleted_profile_restores_local_without_recreating_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = tmp.path().join("meta");
        let instance = tmp.path().join("instance");
        let minecraft = instance.join(crate::storage::MINECRAFT_DIR_NAME);
        std::fs::create_dir_all(minecraft.join("config")).unwrap();
        std::fs::create_dir_all(instance.join("rmcl/content/config/config")).unwrap();
        std::fs::write(minecraft.join("options.txt"), "deleted-profile-options").unwrap();
        std::fs::write(
            instance.join("rmcl/content/config/options.txt"),
            "local-options",
        )
        .unwrap();
        std::fs::write(
            instance.join("rmcl/content/config/config/local.txt"),
            "local",
        )
        .unwrap();

        let selected = switch_profile("inst", Some("deleted"), None, &meta, &instance).unwrap();

        assert_eq!(selected, None);
        assert_eq!(
            std::fs::read_to_string(minecraft.join("options.txt")).unwrap(),
            "local-options"
        );
        assert_eq!(
            std::fs::read_to_string(minecraft.join("config/local.txt")).unwrap(),
            "local"
        );
        assert!(!meta.join("state/profiles/deleted").exists());
    }

    #[test]
    fn switch_between_profiles_saves_old_and_loads_new() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = tmp.path().join("meta");
        let instance = tmp.path().join("instance");
        let minecraft = instance.join(crate::storage::MINECRAFT_DIR_NAME);
        std::fs::create_dir_all(minecraft.join("config")).unwrap();
        std::fs::write(minecraft.join("options.txt"), "changed-a-options").unwrap();
        std::fs::write(minecraft.join("config/a.txt"), "changed-a").unwrap();
        create_profile(&meta, "a").unwrap();
        std::fs::create_dir_all(meta.join("state/profiles/b/config")).unwrap();
        std::fs::write(
            meta.join("state/profiles/b/options.txt"),
            "profile-b-options",
        )
        .unwrap();
        std::fs::write(meta.join("state/profiles/b/config/b.txt"), "profile-b").unwrap();

        let selected = switch_profile("inst", Some("a"), Some("b"), &meta, &instance).unwrap();

        assert_eq!(selected.as_deref(), Some("b"));
        assert_eq!(
            std::fs::read_to_string(meta.join("state/profiles/a/options.txt")).unwrap(),
            "changed-a-options"
        );
        assert_eq!(
            std::fs::read_to_string(meta.join("state/profiles/a/config/a.txt")).unwrap(),
            "changed-a"
        );
        assert_eq!(
            std::fs::read_to_string(minecraft.join("options.txt")).unwrap(),
            "profile-b-options"
        );
        assert_eq!(
            std::fs::read_to_string(minecraft.join("config/b.txt")).unwrap(),
            "profile-b"
        );
        assert!(!minecraft.join("config/a.txt").exists());
    }

    #[test]
    fn second_instance_uses_profile_options_saved_by_first_instance() {
        let tmp = tempfile::tempdir().unwrap();
        let meta = tmp.path().join("meta");
        let first = tmp.path().join("first/minecraft");
        let second_instance = tmp.path().join("second");
        let second = second_instance.join(crate::storage::MINECRAFT_DIR_NAME);
        std::fs::create_dir_all(first.join("config")).unwrap();
        std::fs::create_dir_all(second.join("config")).unwrap();
        std::fs::write(first.join("options.txt"), "first-default").unwrap();
        std::fs::write(second.join("options.txt"), "second-local").unwrap();
        create_profile(&meta, "main").unwrap();

        assert!(prepare(Some("main"), &meta, &first).unwrap());
        std::fs::write(first.join("options.txt"), "changed-in-main").unwrap();
        finish(Some("main"), &meta, &first).unwrap();

        switch_profile("second", None, Some("main"), &meta, &second_instance).unwrap();

        assert_eq!(
            std::fs::read_to_string(second.join("options.txt")).unwrap(),
            "changed-in-main"
        );
        assert_eq!(
            std::fs::read_to_string(second_instance.join("rmcl/content/config/options.txt"))
                .unwrap(),
            "second-local"
        );
    }
}
