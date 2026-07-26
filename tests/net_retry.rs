// integration tests for the retry envelope in src/net/mod.rs. wiremock
// stands in for live upstream APIs so we can assert that 5xx responses
// retry, 4xx responses fail fast, and the cap (MAX_RETRIES = 3, total
// 4 attempts) is honoured. these tests exercise public HttpClient methods,
// not the private retry helper directly.
//
// Tokio time is paused in retrying tests so the production backoff remains
// covered without adding wall-clock delay to the suite.

use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use serde::Deserialize;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use rmcl::net::{HttpClient, download_file};

#[derive(Debug, Deserialize)]
struct ApiResponse {
    ok: bool,
}

fn client_without_timeout() -> HttpClient {
    reqwest::Client::builder().build().unwrap().into()
}

// ---------- get_json (via get_with_retry) ----------

#[tokio::test(start_paused = true)]
async fn get_json_retries_5xx_then_succeeds() {
    let server = MockServer::start().await;
    let attempts = Arc::new(AtomicUsize::new(0));

    // first request: 503. second request: 200.
    Mock::given(method("GET"))
        .and(path("/api"))
        .respond_with(move |_: &wiremock::Request| {
            if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(503)
            } else {
                ResponseTemplate::new(200).set_body_json(json!({"ok": true}))
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let url = format!("{}/api", server.uri());
    let result: ApiResponse = client_without_timeout().get_json(&url).await.unwrap();
    assert!(result.ok);
}

#[tokio::test]
async fn get_json_fails_fast_on_4xx() {
    let server = MockServer::start().await;

    // expect(1) makes wiremock panic on drop if the mock matches more than
    // once. that's how we assert "no retries" without watching the clock.
    Mock::given(method("GET"))
        .and(path("/api"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;

    let url = format!("{}/api", server.uri());
    let err = HttpClient::new()
        .get_json::<ApiResponse>(&url)
        .await
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("404"),
        "expected 404 in error, got: {err:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn get_json_gives_up_after_max_retries() {
    let server = MockServer::start().await;

    // MAX_RETRIES = 3 means we expect exactly 4 attempts before giving up.
    // expect(4) asserts that.
    Mock::given(method("GET"))
        .and(path("/api"))
        .respond_with(ResponseTemplate::new(503))
        .expect(4)
        .mount(&server)
        .await;

    let url = format!("{}/api", server.uri());
    let err = client_without_timeout()
        .get_json::<ApiResponse>(&url)
        .await
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("503"),
        "expected 503 in final error, got: {err:?}"
    );
}

// ---------- download_file ----------

#[tokio::test(start_paused = true)]
async fn download_file_retries_5xx_then_succeeds() {
    let server = MockServer::start().await;
    let attempts = Arc::new(AtomicUsize::new(0));

    Mock::given(method("GET"))
        .and(path("/file.bin"))
        .respond_with(move |_: &wiremock::Request| {
            if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(502)
            } else {
                ResponseTemplate::new(200).set_body_bytes(b"hello, retried".to_vec())
            }
        })
        .expect(2)
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest = tmp.path().join("downloaded.bin");
    let url = format!("{}/file.bin", server.uri());

    download_file(&client_without_timeout(), &url, &dest, |_, _| {})
        .await
        .unwrap();

    let content = std::fs::read(&dest).unwrap();
    assert_eq!(content, b"hello, retried");
}

#[tokio::test]
async fn download_file_fails_fast_on_4xx() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/file.bin"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;

    let tmp = tempfile::tempdir().unwrap();
    let dest: PathBuf = tmp.path().join("never-written.bin");
    let url = format!("{}/file.bin", server.uri());

    let err = download_file(&HttpClient::new(), &url, &dest, |_, _| {})
        .await
        .unwrap_err();
    assert!(
        format!("{err:?}").contains("404"),
        "expected 404 in error, got: {err:?}"
    );
}
