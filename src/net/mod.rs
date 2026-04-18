// networking layer: http client, file downloads, and shared utilities
// for fetching game assets from mojang, mod loaders, and modrinth.

pub mod fabric;
pub mod forge;
pub mod modrinth;
pub mod mojang;
pub mod neoforge;
pub mod quilt;

use reqwest::Client;
use serde::de::DeserializeOwned;
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NetError {
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(String),
    #[error("Server returned error status {status}: {url}")]
    StatusError { status: u16, url: String },
    #[error("Installer process failed: {0}")]
    InstallerFailed(String),
    #[error("Task failed: {0}")]
    TaskFailed(String),
}

#[derive(Clone)]
pub struct HttpClient {
    inner: Client,
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent(format!(
                "mcl/{} (Minecraft Launcher)",
                env!("CARGO_PKG_VERSION")
            ))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self { inner: client }
    }

    pub fn inner(&self) -> &Client {
        &self.inner
    }

    pub async fn get(&self, url: &str) -> Result<reqwest::Response, NetError> {
        let response = self.inner.get(url).send().await?;
        if !response.status().is_success() {
            return Err(NetError::StatusError {
                status: response.status().as_u16(),
                url: url.to_string(),
            });
        }
        Ok(response)
    }

    pub async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, NetError> {
        Ok(self.get(url).await?.json().await?)
    }
}

// streams a file to disk in chunks, calling progress_cb(downloaded, total) along the way.
// total will be 0 if the server doesn't send content-length, so callers
// should handle that gracefully.
pub async fn download_file(
    client: &HttpClient,
    url: &str,
    dest: &Path,
    progress_cb: impl Fn(u64, u64),
) -> Result<(), NetError> {
    use tokio::io::AsyncWriteExt;

    let response = client.get(url).await?;
    let total = response.content_length().unwrap_or(0);

    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mut file = tokio::fs::File::create(dest).await?;
    let mut downloaded: u64 = 0;
    let mut stream = response;

    while let Some(chunk) = stream.chunk().await? {
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        progress_cb(downloaded, total);
    }

    Ok(())
}

// tries JAVA_HOME first, then PATH, then just yolos "java" and hopes for the best
#[must_use]
pub fn detect_java_path() -> String {
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        let java_name = if cfg!(windows) { "java.exe" } else { "java" };
        let bin = std::path::Path::new(&java_home).join("bin").join(java_name);
        if bin.exists() {
            return bin.to_string_lossy().to_string();
        }
    }
    which::which("java")
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "java".to_string())
}

// converts maven coordinates like "org.example:artifact:1.0" into a
// filesystem path like "org/example/artifact/1.0/artifact-1.0.jar".
// supports optional classifier as a 4th component.
#[must_use]
pub fn maven_coord_to_path(coord: &str) -> Option<String> {
    let parts: Vec<&str> = coord.split(':').collect();
    match parts.as_slice() {
        [group, artifact, version] => {
            let group_path = group.replace('.', "/");
            Some(format!(
                "{}/{}/{}/{}-{}.jar",
                group_path, artifact, version, artifact, version
            ))
        }
        [group, artifact, version, classifier] => {
            let group_path = group.replace('.', "/");
            Some(format!(
                "{}/{}/{}/{}-{}-{}.jar",
                group_path, artifact, version, artifact, version, classifier
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maven_3_part_coord() {
        assert_eq!(
            maven_coord_to_path("org.example:artifact:1.0"),
            Some("org/example/artifact/1.0/artifact-1.0.jar".to_string())
        );
    }

    #[test]
    fn maven_4_part_coord_with_classifier() {
        assert_eq!(
            maven_coord_to_path("org.example:artifact:1.0:sources"),
            Some("org/example/artifact/1.0/artifact-1.0-sources.jar".to_string())
        );
    }

    #[test]
    fn maven_nested_group() {
        assert_eq!(
            maven_coord_to_path("com.google.code.gson:gson:2.10"),
            Some("com/google/code/gson/gson/2.10/gson-2.10.jar".to_string())
        );
    }

    #[test]
    fn maven_invalid_too_few_parts() {
        assert_eq!(maven_coord_to_path("org.example:artifact"), None);
    }

    #[test]
    fn maven_invalid_too_many_parts() {
        assert_eq!(maven_coord_to_path("a:b:c:d:e"), None);
    }

    #[test]
    fn maven_invalid_single_part() {
        assert_eq!(maven_coord_to_path("just-a-string"), None);
    }

    #[test]
    fn maven_empty_string() {
        assert_eq!(maven_coord_to_path(""), None);
    }

    #[test]
    fn detect_java_falls_back_to_java() {
        let result = detect_java_path();
        assert!(!result.is_empty());
    }
}
