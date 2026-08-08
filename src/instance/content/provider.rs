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

    async fn search_modpacks(
        &self,
        query: &str,
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

    async fn version(&self, version_id: &str) -> Result<VersionInfo, crate::net::NetError>;

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
            kind,
            query,
            &instance.game_version,
            instance.loader,
            offset,
            limit,
        )
        .await
    }

    async fn search_modpacks(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> Result<DiscoveryResults, crate::net::NetError> {
        crate::net::modrinth::search_modpacks(&self.client, query, offset, limit).await
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
            kind,
            game_version,
            loader,
        )
        .await
    }

    async fn version(&self, version_id: &str) -> Result<VersionInfo, crate::net::NetError> {
        crate::net::modrinth::fetch_version(&self.client, version_id).await
    }

    async fn icon(&self, url: &str) -> Result<Vec<u8>, crate::net::NetError> {
        self.client
            .get_bytes_limited(url, crate::net::MAX_PROVIDER_ASSET_BYTES)
            .await
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

#[derive(Clone)]
pub struct CurseForgeProvider {
    client: crate::net::HttpClient,
    api_key: String,
}

impl CurseForgeProvider {
    pub fn new(client: crate::net::HttpClient, api_key: impl Into<String>) -> Self {
        Self {
            client,
            api_key: api_key.into(),
        }
    }
}

#[async_trait]
impl ContentProvider for CurseForgeProvider {
    fn id(&self) -> &'static str {
        "curseforge"
    }

    async fn search(
        &self,
        kind: ContentKind,
        query: &str,
        instance: &InstanceConfig,
        offset: usize,
        limit: usize,
    ) -> Result<DiscoveryResults, crate::net::NetError> {
        crate::net::curseforge::search_discovery(
            &self.client,
            &self.api_key,
            kind,
            query,
            &instance.game_version,
            instance.loader,
            offset,
            limit,
        )
        .await
    }

    async fn search_modpacks(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> Result<DiscoveryResults, crate::net::NetError> {
        crate::net::curseforge::search_modpacks(&self.client, &self.api_key, query, offset, limit)
            .await
    }

    async fn resolve_files(
        &self,
        files: &[FingerprintQuery],
    ) -> Result<Vec<ResolvedFile>, crate::net::NetError> {
        let mut by_fingerprint = HashMap::<u32, Vec<String>>::new();
        for file in files {
            if let Some(fingerprint) = file
                .fingerprint
                .hash("curseforge")
                .and_then(|value| value.parse().ok())
            {
                by_fingerprint
                    .entry(fingerprint)
                    .or_default()
                    .push(file.key.clone());
            }
        }
        let fingerprints = by_fingerprint.keys().copied().collect::<Vec<_>>();
        let matches = crate::net::curseforge::resolve_fingerprints(
            &self.client,
            &self.api_key,
            &fingerprints,
        )
        .await?;
        Ok(matches
            .into_iter()
            .flat_map(|(fingerprint, project_id, version_id)| {
                by_fingerprint
                    .get(&fingerprint)
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |key| ResolvedFile {
                        key,
                        project: ProviderProject {
                            provider: self.id().to_owned(),
                            project_id: project_id.clone(),
                            version_id: version_id.clone(),
                        },
                    })
            })
            .collect())
    }

    async fn project(
        &self,
        project_id: &str,
    ) -> Result<crate::net::modrinth::ProjectInfo, crate::net::NetError> {
        crate::net::curseforge::fetch_project(&self.client, &self.api_key, project_id).await
    }

    async fn compatible_versions(
        &self,
        project_id: &str,
        kind: ContentKind,
        game_version: &str,
        loader: ModLoader,
    ) -> Result<Vec<VersionInfo>, crate::net::NetError> {
        crate::net::curseforge::fetch_versions(
            &self.client,
            &self.api_key,
            project_id,
            game_version,
            (kind == ContentKind::Mod).then_some(loader),
        )
        .await
    }

    async fn version(&self, version_id: &str) -> Result<VersionInfo, crate::net::NetError> {
        let id = version_id.parse::<u64>().map_err(|_| {
            crate::net::NetError::Parse(format!("Invalid CurseForge file id '{version_id}'"))
        })?;
        crate::net::curseforge::fetch_file_versions(&self.client, &self.api_key, &[id])
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| {
                crate::net::NetError::Parse(format!("CurseForge file '{version_id}' was not found"))
            })
    }

    async fn icon(&self, url: &str) -> Result<Vec<u8>, crate::net::NetError> {
        self.client
            .get_bytes_limited(url, crate::net::MAX_PROVIDER_ASSET_BYTES)
            .await
    }

    async fn download_version(
        &self,
        version: &VersionInfo,
        destination: &std::path::Path,
        installed_path: Option<&std::path::Path>,
    ) -> Result<crate::net::modrinth::DownloadOutcome, crate::net::NetError> {
        let mut version = version.clone();
        crate::net::curseforge::ensure_download_url(&self.client, &self.api_key, &mut version)
            .await?;
        if let Some(installed_path) = installed_path {
            crate::net::modrinth::download_version_file_for_update(
                &self.client,
                &version,
                destination,
                installed_path,
            )
            .await
        } else {
            crate::net::modrinth::download_version_file(&self.client, &version, destination).await
        }
    }
}

pub struct ProviderRegistry {
    providers: Vec<Box<dyn ContentProvider>>,
}

pub(crate) fn has_newer_compatible_version(
    versions: &[VersionInfo],
    installed_version_id: &str,
) -> bool {
    versions
        .iter()
        .position(|version| version.id == installed_version_id)
        .is_some_and(|position| position > 0)
}

impl ProviderRegistry {
    pub(crate) fn new(providers: Vec<Box<dyn ContentProvider>>) -> Self {
        Self { providers }
    }

    pub fn modrinth(client: crate::net::HttpClient) -> Self {
        Self::new(vec![Box::new(ModrinthProvider::new(client))])
    }

    pub fn configured(client: crate::net::HttpClient) -> Self {
        let mut providers: Vec<Box<dyn ContentProvider>> =
            vec![Box::new(ModrinthProvider::new(client.clone()))];
        if let Some(api_key) = crate::net::curseforge::api_key() {
            providers.push(Box::new(CurseForgeProvider::new(client, api_key)));
        }
        Self::new(providers)
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

    pub fn get(&self, id: &str) -> Option<&dyn ContentProvider> {
        self.providers
            .iter()
            .find(|provider| provider.id() == id)
            .map(Box::as_ref)
    }
}

#[cfg(test)]
#[path = "../tests/content/provider.rs"]
mod tests;
