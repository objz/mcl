pub mod fabric;
pub mod forge;
pub mod mojang;
pub mod neoforge;
pub mod quilt;

use reqwest::Client;
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
        match Client::builder()
            .user_agent("mcl/0.1.0 (Minecraft Launcher)")
            .timeout(std::time::Duration::from_secs(30))
            .build()
        {
            Ok(client) => HttpClient { inner: client },
            Err(e) => {
                tracing::error!("Failed to build HTTP client: {}", e);
                HttpClient {
                    inner: Client::new(),
                }
            }
        }
    }

    pub fn inner(&self) -> &Client {
        &self.inner
    }
}

/// Download a file from `url` to `dest`, calling `progress_cb(bytes_downloaded, total_bytes)` as chunks arrive.
/// `total_bytes` is 0 if Content-Length is unknown.
pub async fn download_file(
    client: &HttpClient,
    url: &str,
    dest: &Path,
    progress_cb: impl Fn(u64, u64),
) -> Result<(), NetError> {
    use tokio::io::AsyncWriteExt;

    let response = match client.inner().get(url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("GET {} failed: {}", url, e);
            return Err(NetError::Http(e));
        }
    };

    if !response.status().is_success() {
        let status = response.status().as_u16();
        tracing::error!("HTTP {} for {}", status, url);
        return Err(NetError::StatusError {
            status,
            url: url.to_string(),
        });
    }

    let total = response.content_length().unwrap_or(0);

    if let Some(parent) = dest.parent() {
        match tokio::fs::create_dir_all(parent).await {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("Failed to create directory {}: {}", parent.display(), e);
                return Err(NetError::Io(e));
            }
        }
    }

    let mut file = match tokio::fs::File::create(dest).await {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("Failed to create file {}: {}", dest.display(), e);
            return Err(NetError::Io(e));
        }
    };

    let mut downloaded: u64 = 0;
    let mut stream = response;

    loop {
        match stream.chunk().await {
            Ok(Some(chunk)) => {
                match file.write_all(&chunk).await {
                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!("Failed to write chunk to {}: {}", dest.display(), e);
                        return Err(NetError::Io(e));
                    }
                }
                downloaded += chunk.len() as u64;
                progress_cb(downloaded, total);
            }
            Ok(None) => break,
            Err(e) => {
                tracing::error!("Failed to read response chunk from {}: {}", url, e);
                return Err(NetError::Http(e));
            }
        }
    }

    Ok(())
}

pub fn detect_java_path() -> String {
    if let Ok(java_home) = std::env::var("JAVA_HOME") {
        let path = std::path::Path::new(&java_home).join("bin").join("java");
        if path.exists() {
            return path.to_string_lossy().to_string();
        }
    }
    "java".to_string()
}

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
