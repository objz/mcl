use std::collections::{BTreeMap, HashMap};
use std::io::Read;
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
            let Resolution::Resolved { project } = &record.resolution else {
                return None;
            };
            (project.provider == provider && project.project_id == project_id)
                .then(|| minecraft_dir.join(&record.relative_path))
        })
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
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        sha1.update(&buffer[..read]);
        sha512.update(&buffer[..read]);
    }
    let hashes = BTreeMap::from([
        ("sha1".to_owned(), format!("{:x}", sha1.finalize())),
        ("sha512".to_owned(), format!("{:x}", sha512.finalize())),
    ]);
    Ok(FileFingerprint {
        size: metadata.len(),
        modified_ns,
        hashes,
    })
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
