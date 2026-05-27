// integration tests for the public mojang fetchers. wiremock stands in for
// Mojang so tests are fast, deterministic, and don't depend on the live
// endpoint. these are different from the #[ignore = "hits live Mojang API"]
// tests in src/net/mojang.rs which verify the upstream schema hasn't drifted;
// these here verify our parsing + retry envelope on synthetic responses.

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use rmcl::net::HttpClient;
use rmcl::net::mojang::{VersionEntry, fetch_version_manifest_from, fetch_version_meta_with_raw};

fn synthetic_manifest() -> serde_json::Value {
    json!({
        "latest": { "release": "1.20.1", "snapshot": "24w01a" },
        "versions": [
            {
                "id": "1.20.1",
                "type": "release",
                "url": "https://example.com/1.20.1.json",
                "sha1": "0000000000000000000000000000000000000000"
            },
            {
                "id": "1.7.10",
                "type": "release",
                "url": "https://example.com/1.7.10.json",
                "sha1": "0000000000000000000000000000000000000000"
            }
        ]
    })
}

fn synthetic_version_meta() -> serde_json::Value {
    json!({
        "id": "1.20.1",
        "mainClass": "net.minecraft.client.main.Main",
        "assetIndex": {
            "id": "5",
            "url": "https://example.com/assets/5.json",
            "sha1": "0000000000000000000000000000000000000000"
        },
        "downloads": {
            "client": {
                "url": "https://example.com/client.jar",
                "sha1": "0000000000000000000000000000000000000000",
                "size": 12345
            }
        },
        "libraries": [],
        "javaVersion": { "majorVersion": 17 }
    })
}

#[tokio::test]
async fn fetch_version_manifest_parses_synthetic_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/manifest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(synthetic_manifest()))
        .expect(1)
        .mount(&server)
        .await;

    let url = format!("{}/manifest.json", server.uri());
    let manifest = fetch_version_manifest_from(&HttpClient::new(), &url)
        .await
        .expect("manifest");

    assert_eq!(manifest.latest.release, "1.20.1");
    assert_eq!(manifest.latest.snapshot, "24w01a");
    assert_eq!(manifest.versions.len(), 2);
    assert_eq!(manifest.versions[0].id, "1.20.1");
    assert_eq!(manifest.versions[1].id, "1.7.10");
}

#[tokio::test]
async fn fetch_version_meta_returns_struct_and_raw_bytes() {
    let server = MockServer::start().await;
    let body_json = synthetic_version_meta();
    // serialise once so we can assert the raw bytes equal what the mock
    // actually sent (wiremock re-serialises the json, so we have to match
    // its output format)
    Mock::given(method("GET"))
        .and(path("/1.20.1.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body_json.clone()))
        .expect(1)
        .mount(&server)
        .await;

    let entry = VersionEntry {
        id: "1.20.1".to_string(),
        version_type: "release".to_string(),
        url: format!("{}/1.20.1.json", server.uri()),
        sha1: "0".repeat(40),
    };

    let (meta, raw) = fetch_version_meta_with_raw(&HttpClient::new(), &entry)
        .await
        .expect("meta");

    assert_eq!(meta.id, "1.20.1");
    assert_eq!(meta.main_class, "net.minecraft.client.main.Main");
    assert_eq!(meta.asset_index.id, "5");
    assert_eq!(meta.downloads.client.size, 12345);
    assert_eq!(meta.java_version.unwrap().major_version, 17);

    // the raw bytes must parse back to the same struct - verifies the
    // get_json_with_raw plumbing actually captures the upstream body intact.
    let reparsed: serde_json::Value = serde_json::from_slice(&raw).expect("raw is json");
    assert_eq!(reparsed["id"], "1.20.1");
    assert_eq!(reparsed["mainClass"], "net.minecraft.client.main.Main");
}

#[tokio::test]
async fn fetch_version_manifest_retries_5xx_and_succeeds() {
    let server = MockServer::start().await;

    // one transient 503 then the real payload; covers the integration of the
    // retry envelope (already unit-tested in net_retry.rs) with the actual
    // VersionManifest deserialisation path.
    Mock::given(method("GET"))
        .and(path("/manifest.json"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/manifest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(synthetic_manifest()))
        .expect(1)
        .mount(&server)
        .await;

    let url = format!("{}/manifest.json", server.uri());
    let manifest = fetch_version_manifest_from(&HttpClient::new(), &url)
        .await
        .expect("manifest after retry");
    assert_eq!(manifest.versions.len(), 2);
}
