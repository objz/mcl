// networking layer: http client, file downloads, and shared utilities
// for fetching game assets from mojang, mod loaders, and modrinth.

pub mod curseforge;
pub mod fabric;
pub mod forge;
pub mod modrinth;
pub mod mojang;
pub mod neoforge;
pub mod quilt;

use reqwest::Client;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::Path;
use thiserror::Error;

pub const MAX_PROVIDER_ASSET_BYTES: usize = 16 * 1024 * 1024;

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

impl From<Client> for HttpClient {
    fn from(inner: Client) -> Self {
        Self { inner }
    }
}

impl NetError {
    pub(crate) fn is_retryable(&self) -> bool {
        matches!(self, Self::StatusError { status: 429, .. }) || is_retryable(self)
    }
}

impl HttpClient {
    pub fn new() -> Self {
        let user_agent = format!("rmcl/{} (Minecraft Launcher)", env!("CARGO_PKG_VERSION"));
        let client = Client::builder()
            .user_agent(user_agent.clone())
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(
                    "Failed to build configured HTTP client, falling back to reqwest default: {}",
                    e
                );
                Client::new()
            });
        tracing::trace!("Created HTTP client with user-agent '{}'", user_agent);
        Self { inner: client }
    }

    pub fn inner(&self) -> &Client {
        &self.inner
    }

    pub async fn get(&self, url: &str) -> Result<reqwest::Response, NetError> {
        tracing::trace!("HTTP GET {}", url);
        let response = self.inner.get(url).send().await?;
        if !response.status().is_success() {
            tracing::debug!(
                "HTTP GET {} returned non-success status {}",
                url,
                response.status()
            );
            return Err(NetError::StatusError {
                status: response.status().as_u16(),
                url: url.to_string(),
            });
        }
        tracing::trace!("HTTP GET {} succeeded with {}", url, response.status());
        Ok(response)
    }

    pub async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, NetError> {
        get_with_retry(self, url, |resp| async move { Ok(resp.json().await?) }).await
    }

    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, NetError> {
        get_with_retry(
            self,
            url,
            |resp| async move { Ok(resp.bytes().await?.to_vec()) },
        )
        .await
    }

    pub async fn get_bytes_limited(&self, url: &str, limit: usize) -> Result<Vec<u8>, NetError> {
        get_with_retry(self, url, move |mut response| async move {
            if response
                .content_length()
                .is_some_and(|length| length > limit as u64)
            {
                return Err(NetError::Parse(format!(
                    "Response exceeds the {limit}-byte limit"
                )));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                if bytes.len().saturating_add(chunk.len()) > limit {
                    return Err(NetError::Parse(format!(
                        "Response exceeds the {limit}-byte limit"
                    )));
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        })
        .await
    }

    pub async fn post_json<B, T>(&self, url: &str, body: &B) -> Result<T, NetError>
    where
        B: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        for attempt in 0..=MAX_RETRIES {
            tracing::trace!("HTTP POST {}", url);
            let result = async {
                let response = self.inner.post(url).json(body).send().await?;
                if !response.status().is_success() {
                    return Err(NetError::StatusError {
                        status: response.status().as_u16(),
                        url: url.to_owned(),
                    });
                }
                Ok(response.json().await?)
            }
            .await;
            match result {
                Ok(value) => return Ok(value),
                Err(error) if is_retryable(&error) && attempt < MAX_RETRIES => {
                    sleep_before_retry("request", url, attempt, &error).await;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("retry loop returns on success or final error")
    }

    // fetch JSON and also keep the raw bytes. used by install paths that
    // want both the parsed shape (for downloading libraries from it) and
    // the original bytes (to write byte-for-byte to the loader-profiles
    // cache, so any field we don't know about survives).
    pub async fn get_json_with_raw<T: DeserializeOwned>(
        &self,
        url: &str,
        label: &str,
    ) -> Result<(T, Vec<u8>), NetError> {
        tracing::debug!("Fetching {} JSON from {}", label, url);
        let raw = self.get_bytes(url).await?;
        tracing::trace!("Fetched {} byte(s) for {}", raw.len(), label);
        let parsed: T = serde_json::from_slice(&raw)
            .map_err(|e| NetError::Parse(format!("Failed to parse {label}: {e}")))?;
        Ok((parsed, raw))
    }
}

// shared retry envelope around `client.get(url).await? -> decode`. retries
// transient failures (timeouts, connect errors, 5xx) with exponential
// backoff. used by both get_json and get_bytes.
async fn get_with_retry<T, F, Fut>(client: &HttpClient, url: &str, decode: F) -> Result<T, NetError>
where
    F: Fn(reqwest::Response) -> Fut,
    Fut: std::future::Future<Output = Result<T, NetError>>,
{
    for attempt in 0..=MAX_RETRIES {
        match client.get(url).await {
            Ok(resp) => match decode(resp).await {
                Ok(value) => return Ok(value),
                Err(e) if is_retryable(&e) => {
                    if attempt == MAX_RETRIES {
                        return Err(e);
                    }
                    sleep_before_retry("request", url, attempt, &e).await;
                }
                Err(e) => return Err(e),
            },
            Err(e) if is_retryable(&e) => {
                if attempt == MAX_RETRIES {
                    return Err(e);
                }
                sleep_before_retry("request", url, attempt, &e).await;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("retry loop returns on success or final error")
}

const MAX_RETRIES: u32 = 3;
const RETRY_BASE_DELAY_MS: u64 = 500;

async fn sleep_before_retry(kind: &str, url: &str, attempt: u32, err: &NetError) {
    let delay = RETRY_BASE_DELAY_MS * 2u64.pow(attempt);
    tracing::debug!(
        "{} failed, retrying after {}ms (attempt {}/{}): {}: {}",
        kind,
        delay,
        attempt + 2,
        MAX_RETRIES + 1,
        url,
        err
    );
    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
}

// streams a file to disk in chunks, calling progress_cb(downloaded, total) along the way.
// total will be 0 if the server doesn't send content-length, so callers
// should handle that gracefully. retries transient failures with exponential backoff.
pub async fn download_file(
    client: &HttpClient,
    url: &str,
    dest: &Path,
    progress_cb: impl Fn(u64, u64),
) -> Result<(), NetError> {
    tracing::debug!("Downloading {} to {}", url, dest.display());

    let result = 'download: {
        for attempt in 0..=MAX_RETRIES {
            match download_file_once(client, url, dest, &progress_cb).await {
                Ok(()) => {
                    tracing::debug!("Downloaded {} to {}", url, dest.display());
                    break 'download Ok(());
                }
                Err(e) if is_retryable(&e) => {
                    if attempt == MAX_RETRIES {
                        break 'download Err(e);
                    }
                    sleep_before_retry("download", url, attempt, &e).await;
                }
                Err(e) => break 'download Err(e),
            }
        }
        unreachable!("retry loop returns on success or final error")
    };
    if result.is_err() {
        let _ = tokio::fs::remove_file(dest).await;
    }
    result
}

// single attempt at downloading a file to disk
async fn download_file_once(
    client: &HttpClient,
    url: &str,
    dest: &Path,
    progress_cb: &impl Fn(u64, u64),
) -> Result<(), NetError> {
    use tokio::io::AsyncWriteExt;

    let response = client.get(url).await?;
    let total = response.content_length().unwrap_or(0);
    tracing::trace!("Download content length for {}: {}", url, total);

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
    file.flush().await?;

    Ok(())
}

// body decode errors and timeouts are worth retrying, but a 404 or disk
// error isn't. Parse errors stay non-retryable: by the time we hit one
// the response body has fully arrived, so the failure means the upstream
// returned malformed JSON - retrying won't fix that.
fn is_retryable(err: &NetError) -> bool {
    match err {
        NetError::Http(e) => e.is_timeout() || e.is_body() || e.is_connect(),
        NetError::StatusError { status, .. } => *status >= 500,
        _ => false,
    }
}
