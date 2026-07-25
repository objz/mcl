use std::path::{Path, PathBuf};

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
        self.state().join("migration-v2.json")
    }

    pub fn cache_rebuild_pending(&self) -> PathBuf {
        self.state().join("cache-rebuild.pending")
    }
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
    }
}
