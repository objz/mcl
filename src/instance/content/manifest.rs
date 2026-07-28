use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha1::Digest as _;

const MANIFEST_VERSION: u32 = 1;
static MANIFEST_LOCKS: LazyLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentKind {
    Mod,
    ResourcePack,
    Shader,
}

impl ContentKind {
    pub fn directory(self) -> &'static str {
        match self {
            Self::Mod => "mods",
            Self::ResourcePack => "resourcepacks",
            Self::Shader => "shaderpacks",
        }
    }

    pub fn unavailable_message(self, loader: crate::instance::ModLoader) -> Option<&'static str> {
        if loader != crate::instance::ModLoader::Vanilla {
            return None;
        }
        match self {
            Self::Mod => Some("Vanilla does not support mods."),
            Self::Shader => Some("Vanilla does not support shaders."),
            Self::ResourcePack => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderProject {
    pub provider: String,
    pub project_id: String,
    pub version_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Resolution {
    #[default]
    Pending,
    Resolved {
        project: ProviderProject,
    },
    Unmatched {
        checked_at: i64,
        providers: Vec<String>,
    },
    Ambiguous {
        candidates: Vec<ProviderProject>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFingerprint {
    pub size: u64,
    pub modified_ns: u128,
    pub hashes: BTreeMap<String, String>,
}

impl FileFingerprint {
    pub fn hash(&self, algorithm: &str) -> Option<&str> {
        self.hashes.get(algorithm).map(String::as_str)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentFileRecord {
    pub relative_path: PathBuf,
    pub kind: ContentKind,
    pub enabled: bool,
    pub fingerprint: FileFingerprint,
    #[serde(default)]
    pub resolution: Resolution,
    #[serde(default)]
    pub provider_aliases: Vec<ProviderProject>,
    #[serde(default)]
    pub required_dependencies: Vec<ProviderProject>,
    #[serde(default)]
    pub automatic_dependency: bool,
    #[serde(default)]
    pub cleanup_eligible: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentManifest {
    pub version: u32,
    #[serde(default)]
    pub files: Vec<ContentFileRecord>,
}

impl Default for ContentManifest {
    fn default() -> Self {
        Self {
            version: MANIFEST_VERSION,
            files: Vec::new(),
        }
    }
}

impl ContentManifest {
    pub fn load(path: &Path) -> Result<Self, ManifestError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(path)?;
        let manifest: Self = serde_json::from_slice(&bytes)?;
        if manifest.version != MANIFEST_VERSION {
            return Err(ManifestError::UnsupportedVersion(manifest.version));
        }
        Ok(manifest)
    }

    pub fn save(&self, path: &Path) -> Result<(), ManifestError> {
        let parent = path.parent().ok_or_else(|| {
            ManifestError::InvalidPath(format!("{} has no parent", path.display()))
        })?;
        std::fs::create_dir_all(parent)?;
        let bytes = serde_json::to_vec_pretty(self)?;
        crate::storage::write_atomic(path, &bytes)?;
        Ok(())
    }

    pub fn update<T>(
        path: &Path,
        update: impl FnOnce(&mut Self) -> Result<T, ManifestError>,
    ) -> Result<T, ManifestError> {
        let lock = {
            let mut locks = MANIFEST_LOCKS
                .lock()
                .map_err(|_| ManifestError::LockPoisoned)?;
            locks
                .entry(path.to_owned())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().map_err(|_| ManifestError::LockPoisoned)?;
        let mut manifest = Self::load(path)?;
        let result = update(&mut manifest)?;
        manifest.save(path)?;
        Ok(result)
    }

    pub fn record(&self, relative_path: &Path) -> Option<&ContentFileRecord> {
        self.files
            .iter()
            .find(|record| record.relative_path == relative_path)
    }

    pub fn upsert(&mut self, record: ContentFileRecord) {
        if let Some(existing) = self
            .files
            .iter_mut()
            .find(|existing| existing.relative_path == record.relative_path)
        {
            *existing = record;
        } else {
            self.files.push(record);
            self.files
                .sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
        }
    }

    pub fn remove(&mut self, relative_path: &Path) {
        self.files
            .retain(|record| record.relative_path != relative_path);
    }

    pub fn rename_record(&mut self, from: &Path, to: &Path, enabled: bool) -> bool {
        let Some(index) = self
            .files
            .iter()
            .position(|record| record.relative_path == from || record.relative_path == to)
        else {
            return false;
        };
        let mut record = self.files.remove(index);
        record.relative_path = to.to_owned();
        record.enabled = enabled;
        self.upsert(record);
        true
    }

    pub fn resolved_project_path(
        &self,
        provider: &str,
        project_id: &str,
        minecraft_dir: &Path,
    ) -> Option<PathBuf> {
        self.files.iter().find_map(|record| {
            record
                .matches_project(provider, project_id)
                .then(|| minecraft_dir.join(&record.relative_path))
        })
    }

    pub fn resolved_project_record(
        &self,
        provider: &str,
        project_id: &str,
    ) -> Option<&ContentFileRecord> {
        self.files
            .iter()
            .find(|record| record.matches_project(provider, project_id))
    }

    pub fn dependent_paths(&self, relative_path: &Path) -> Vec<PathBuf> {
        let Some(target) = self.record(relative_path) else {
            return Vec::new();
        };
        self.files
            .iter()
            .filter(|record| record.relative_path != relative_path)
            .filter(|record| {
                record.required_dependencies.iter().any(|dependency| {
                    target.matches_project(&dependency.provider, &dependency.project_id)
                })
            })
            .map(|record| record.relative_path.clone())
            .collect()
    }

    pub fn orphaned_dependencies_after_removing(&self, relative_path: &Path) -> Vec<PathBuf> {
        self.orphaned_dependencies_with_removed(std::iter::once(relative_path.to_owned()))
    }

    pub fn orphaned_dependencies(&self) -> Vec<PathBuf> {
        self.orphaned_dependencies_with_removed(std::iter::empty())
    }

    fn orphaned_dependencies_with_removed(
        &self,
        removed: impl IntoIterator<Item = PathBuf>,
    ) -> Vec<PathBuf> {
        let mut removed = removed
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let mut orphans = Vec::new();
        loop {
            let mut found = false;
            for candidate in &self.files {
                if !candidate.automatic_dependency
                    || !candidate.cleanup_eligible
                    || removed.contains(&candidate.relative_path)
                {
                    continue;
                }
                let still_required = self
                    .files
                    .iter()
                    .filter(|record| !removed.contains(&record.relative_path))
                    .any(|record| {
                        record.required_dependencies.iter().any(|dependency| {
                            candidate.matches_project(&dependency.provider, &dependency.project_id)
                        })
                    });
                if !still_required {
                    removed.insert(candidate.relative_path.clone());
                    orphans.push(candidate.relative_path.clone());
                    found = true;
                }
            }
            if !found {
                break;
            }
        }
        orphans
    }
}

impl ContentFileRecord {
    pub fn resolved_project(&self) -> Option<&ProviderProject> {
        match &self.resolution {
            Resolution::Resolved { project } => Some(project),
            _ => None,
        }
    }

    pub fn matches_project(&self, provider: &str, project_id: &str) -> bool {
        self.resolved_project()
            .into_iter()
            .chain(self.provider_aliases.iter())
            .any(|project| project.provider == provider && project.project_id == project_id)
    }

    pub fn project_for_provider(
        &self,
        provider: &str,
        project_id: &str,
    ) -> Option<&ProviderProject> {
        self.resolved_project()
            .into_iter()
            .chain(self.provider_aliases.iter())
            .find(|project| project.provider == provider && project.project_id == project_id)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Unsupported content manifest version {0}")]
    UnsupportedVersion(u32),
    #[error("Invalid manifest path: {0}")]
    InvalidPath(String),
    #[error("Content manifest lock was poisoned")]
    LockPoisoned,
}

pub fn fingerprint(path: &Path) -> Result<FileFingerprint, std::io::Error> {
    let metadata = std::fs::metadata(path)?;
    let modified_ns = metadata
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut file = std::fs::File::open(path)?;
    let mut sha1 = sha1::Sha1::new();
    let mut sha512 = sha2::Sha512::new();
    let mut curseforge_len = 0u32;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        sha1.update(&buffer[..read]);
        sha512.update(&buffer[..read]);
        curseforge_len = curseforge_len.saturating_add(
            buffer[..read]
                .iter()
                .filter(|byte| !matches!(byte, b'\t' | b'\n' | b'\r' | b' '))
                .count() as u32,
        );
    }
    file.rewind()?;
    let hashes = BTreeMap::from([
        ("sha1".to_owned(), format!("{:x}", sha1.finalize())),
        ("sha512".to_owned(), format!("{:x}", sha512.finalize())),
        (
            "curseforge".to_owned(),
            curseforge_fingerprint(&mut file, curseforge_len)?.to_string(),
        ),
    ]);
    Ok(FileFingerprint {
        size: metadata.len(),
        modified_ns,
        hashes,
    })
}

fn curseforge_fingerprint(reader: &mut impl Read, length: u32) -> std::io::Result<u32> {
    const M: u32 = 0x5bd1e995;
    let mut hash = 1 ^ length;
    let mut block = [0u8; 4];
    let mut block_len = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        for &byte in &buffer[..read] {
            if matches!(byte, b'\t' | b'\n' | b'\r' | b' ') {
                continue;
            }
            block[block_len] = byte;
            block_len += 1;
            if block_len == 4 {
                let mut value = u32::from_le_bytes(block);
                value = value.wrapping_mul(M);
                value ^= value >> 24;
                value = value.wrapping_mul(M);
                hash = hash.wrapping_mul(M) ^ value;
                block_len = 0;
            }
        }
    }
    match block_len {
        3 => {
            hash ^= u32::from(block[2]) << 16;
            hash ^= u32::from(block[1]) << 8;
            hash ^= u32::from(block[0]);
            hash = hash.wrapping_mul(M);
        }
        2 => {
            hash ^= u32::from(block[1]) << 8;
            hash ^= u32::from(block[0]);
            hash = hash.wrapping_mul(M);
        }
        1 => {
            hash ^= u32::from(block[0]);
            hash = hash.wrapping_mul(M);
        }
        _ => {}
    }
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(M);
    hash ^= hash >> 15;
    Ok(hash)
}

pub fn fingerprint_metadata(path: &Path) -> Result<FileFingerprint, std::io::Error> {
    let metadata = std::fs::metadata(path)?;
    let modified_ns = metadata
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(FileFingerprint {
        size: metadata.len(),
        modified_ns,
        hashes: BTreeMap::new(),
    })
}

#[cfg(test)]
#[path = "../tests/content/manifest.rs"]
mod tests;
