pub mod fabric;
pub mod forge;
pub mod mojang;
pub mod neoforge;
pub mod quilt;

use std::path::Path;
use reqwest::Client;
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
                HttpClient { inner: Client::new() }
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
        return Err(NetError::StatusError { status, url: url.to_string() });
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
