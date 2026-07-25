use std::collections::HashMap;

use async_trait::async_trait;

use crate::instance::content::manifest::{ContentKind, FileFingerprint, ProviderProject};
use crate::instance::{InstanceConfig, ModLoader};
use crate::net::modrinth::{DiscoveryResults, VersionInfo};

#[derive(Debug, Clone)]
pub struct FingerprintQuery {
    pub key: String,
    pub kind: ContentKind,
    pub fingerprint: FileFingerprint,
}

#[derive(Debug, Clone)]
pub struct ResolvedFile {
    pub key: String,
    pub project: ProviderProject,
}

#[async_trait]
pub trait ContentProvider: Send + Sync {
    fn id(&self) -> &'static str;

    async fn search(
        &self,
        kind: ContentKind,
        query: &str,
        instance: &InstanceConfig,
        offset: usize,
        limit: usize,
    ) -> Result<DiscoveryResults, crate::net::NetError>;

    async fn resolve_files(
        &self,
        files: &[FingerprintQuery],
    ) -> Result<Vec<ResolvedFile>, crate::net::NetError>;

    async fn project(
        &self,
        project_id: &str,
    ) -> Result<crate::net::modrinth::ProjectInfo, crate::net::NetError>;

    async fn compatible_versions(
        &self,
        project_id: &str,
        kind: ContentKind,
        game_version: &str,
        loader: ModLoader,
    ) -> Result<Vec<VersionInfo>, crate::net::NetError>;

    async fn icon(&self, url: &str) -> Result<Vec<u8>, crate::net::NetError>;

    async fn download_version(
        &self,
        version: &VersionInfo,
        destination: &std::path::Path,
        installed_path: Option<&std::path::Path>,
    ) -> Result<crate::net::modrinth::DownloadOutcome, crate::net::NetError>;
}

#[derive(Clone)]
pub struct ModrinthProvider {
    client: crate::net::HttpClient,
}

impl ModrinthProvider {
    pub fn new(client: crate::net::HttpClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl ContentProvider for ModrinthProvider {
    fn id(&self) -> &'static str {
        "modrinth"
    }

    async fn search(
        &self,
        kind: ContentKind,
        query: &str,
        instance: &InstanceConfig,
        offset: usize,
        limit: usize,
    ) -> Result<DiscoveryResults, crate::net::NetError> {
        crate::net::modrinth::search_discovery(
            &self.client,
            discovery_kind(kind),
            query,
            &instance.game_version,
            instance.loader,
            offset,
            limit,
        )
        .await
    }

    async fn resolve_files(
        &self,
        files: &[FingerprintQuery],
    ) -> Result<Vec<ResolvedFile>, crate::net::NetError> {
        let mut by_hash = HashMap::<String, Vec<String>>::new();
        for file in files {
            if let Some(hash) = file.fingerprint.hash("sha512") {
                by_hash
                    .entry(hash.to_owned())
                    .or_default()
                    .push(file.key.clone());
            }
        }
        let hashes = by_hash.keys().cloned().collect::<Vec<_>>();
        let versions =
            crate::net::modrinth::resolve_version_files(&self.client, &hashes, "sha512").await?;
        Ok(versions
            .into_iter()
            .flat_map(|(hash, version)| {
                let keys = by_hash.get(&hash).cloned().unwrap_or_default();
                if version.project_id.is_empty() {
                    return Vec::new();
                }
                keys.into_iter()
                    .map(|key| ResolvedFile {
                        key,
                        project: ProviderProject {
                            provider: self.id().to_owned(),
                            project_id: version.project_id.clone(),
                            version_id: version.id.clone(),
                        },
                    })
                    .collect()
            })
            .collect())
    }

    async fn project(
        &self,
        project_id: &str,
    ) -> Result<crate::net::modrinth::ProjectInfo, crate::net::NetError> {
        crate::net::modrinth::fetch_project(&self.client, project_id).await
    }

    async fn compatible_versions(
        &self,
        project_id: &str,
        kind: ContentKind,
        game_version: &str,
        loader: ModLoader,
    ) -> Result<Vec<VersionInfo>, crate::net::NetError> {
        crate::net::modrinth::fetch_content_versions(
            &self.client,
            project_id,
            discovery_kind(kind),
            game_version,
            loader,
        )
        .await
    }

    async fn icon(&self, url: &str) -> Result<Vec<u8>, crate::net::NetError> {
        self.client.get_bytes(url).await
    }

    async fn download_version(
        &self,
        version: &VersionInfo,
        destination: &std::path::Path,
        installed_path: Option<&std::path::Path>,
    ) -> Result<crate::net::modrinth::DownloadOutcome, crate::net::NetError> {
        if let Some(installed_path) = installed_path {
            crate::net::modrinth::download_version_file_for_update(
                &self.client,
                version,
                destination,
                installed_path,
            )
            .await
        } else {
            crate::net::modrinth::download_version_file(&self.client, version, destination).await
        }
    }
}

fn discovery_kind(kind: ContentKind) -> crate::net::modrinth::DiscoveryKind {
    match kind {
        ContentKind::Mod => crate::net::modrinth::DiscoveryKind::Mod,
        ContentKind::ResourcePack => crate::net::modrinth::DiscoveryKind::ResourcePack,
        ContentKind::Shader => crate::net::modrinth::DiscoveryKind::Shader,
    }
}

pub struct ProviderRegistry {
    providers: Vec<Box<dyn ContentProvider>>,
}

impl ProviderRegistry {
    pub fn modrinth(client: crate::net::HttpClient) -> Self {
        Self {
            providers: vec![Box::new(ModrinthProvider::new(client))],
        }
    }

    pub fn providers(&self) -> &[Box<dyn ContentProvider>] {
        &self.providers
    }

    pub fn preferred(&self, id: &str) -> Option<&dyn ContentProvider> {
        self.providers
            .iter()
            .find(|provider| provider.id() == id)
            .map(Box::as_ref)
            .or_else(|| self.providers.first().map(Box::as_ref))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_falls_back_to_first_capable_provider() {
        let registry = ProviderRegistry::modrinth(crate::net::HttpClient::new());
        assert_eq!(registry.preferred("unknown").unwrap().id(), "modrinth");
    }
}
