use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_FILE_ID: AtomicU64 = AtomicU64::new(1);

pub const MINECRAFT_DIR_NAME: &str = "minecraft";
pub const INSTANCE_STATE_DIR_NAME: &str = "rmcl";
pub const LAYOUT_VERSION: u32 = 2;

#[derive(Debug, Clone)]
pub struct InstancePaths {
    root: PathBuf,
}

impl InstancePaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn minecraft(&self) -> PathBuf {
        self.root.join(MINECRAFT_DIR_NAME)
    }

    pub fn state(&self) -> PathBuf {
        self.root.join(INSTANCE_STATE_DIR_NAME)
    }

    pub fn content(&self) -> PathBuf {
        self.state().join("content")
    }

    pub fn content_manifest(&self) -> PathBuf {
        self.content().join("manifest.json")
    }

    pub fn local_config(&self) -> PathBuf {
        self.content().join("config")
    }
}

#[derive(Debug, Clone)]
pub struct MetadataPaths {
    root: PathBuf,
}

impl MetadataPaths {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn state(&self) -> PathBuf {
        self.root.join("state")
    }

    pub fn profiles(&self) -> PathBuf {
        self.state().join("profiles")
    }

    pub fn backups(&self) -> PathBuf {
        self.state().join("backups")
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn minecraft_cache(&self) -> PathBuf {
        self.cache().join("minecraft")
    }

    pub fn versions(&self) -> PathBuf {
        self.minecraft_cache().join("versions")
    }

    pub fn libraries(&self) -> PathBuf {
        self.minecraft_cache().join("libraries")
    }

    pub fn assets(&self) -> PathBuf {
        self.minecraft_cache().join("assets")
    }

    pub fn loader_profiles(&self) -> PathBuf {
        self.cache().join("loaders").join("profiles")
    }

    pub fn provider_cache(&self, provider: &str) -> PathBuf {
        self.cache().join("providers").join(provider)
    }

    pub fn provider_projects(&self, provider: &str) -> PathBuf {
        self.provider_cache(provider).join("projects")
    }

    pub fn provider_versions(&self, provider: &str) -> PathBuf {
        self.provider_cache(provider).join("versions")
    }

    pub fn provider_icons(&self, provider: &str) -> PathBuf {
        self.provider_cache(provider).join("icons")
    }

    pub fn temporary(&self) -> PathBuf {
        self.root.join("tmp")
    }

    pub fn layout_marker(&self) -> PathBuf {
        self.state().join("layout.json")
    }

    pub fn migration_journal(&self) -> PathBuf {
        self.state().join("migration.json")
    }

    pub fn cache_rebuild_pending(&self) -> PathBuf {
        self.state().join("cache-rebuild.pending")
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let id = TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("data");
    let temporary = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), id));
    let result = (|| {
        use std::io::Write;

        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    if !destination.exists() {
        return std::fs::rename(source, destination);
    }
    let id = TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed);
    let backup = destination.with_extension(format!("rmcl-replaced-{id}"));
    std::fs::rename(destination, &backup)?;
    if let Err(error) = std::fs::rename(source, destination) {
        let _ = std::fs::rename(&backup, destination);
        return Err(error);
    }
    std::fs::remove_file(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_paths_use_visible_directories() {
        let paths = InstancePaths::new("/instances/example");
        assert_eq!(
            paths.minecraft(),
            PathBuf::from("/instances/example/minecraft")
        );
        assert_eq!(
            paths.content_manifest(),
            PathBuf::from("/instances/example/rmcl/content/manifest.json")
        );
        assert_eq!(
            paths.local_config(),
            PathBuf::from("/instances/example/rmcl/content/config")
        );
    }

    #[test]
    fn metadata_paths_separate_state_and_cache() {
        let paths = MetadataPaths::new("/meta");
        assert_eq!(paths.profiles(), PathBuf::from("/meta/state/profiles"));
        assert_eq!(
            paths.provider_icons("modrinth"),
            PathBuf::from("/meta/cache/providers/modrinth/icons")
        );
        assert_eq!(
            paths.versions(),
            PathBuf::from("/meta/cache/minecraft/versions")
        );
        assert_eq!(
            paths.migration_journal(),
            PathBuf::from("/meta/state/migration.json")
        );
    }

    #[test]
    fn atomic_write_replaces_existing_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.json");
        std::fs::write(&path, b"old").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"new");
    }
}
