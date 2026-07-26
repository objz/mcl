// integration tests for the public mojang fetchers. wiremock stands in for
// Mojang so tests are fast, deterministic, and don't depend on the live
// endpoint. these are different from the #[ignore = "hits live Mojang API"]
// tests in tests/live_apis.rs which verify the upstream schema hasn't drifted;
// these here verify our parsing + retry envelope on synthetic responses.

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use rmcl::net::HttpClient;
use rmcl::net::mojang::{
    Artifact, AssetIndex, Download, JavaVersion, Library, LibraryDownloads, VersionDownloads,
    VersionEntry, VersionMeta, download_assets, download_assets_from, download_libraries,
    fetch_version_manifest_from, fetch_version_meta_with_raw,
};

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

// constructs a minimal VersionMeta with a single library pointing at the
// wiremock server. lets download_libraries hit synthetic URLs without any
// production-side URL override.
fn meta_with_one_library(server_uri: &str) -> VersionMeta {
    VersionMeta {
        id: "test".to_string(),
        main_class: "net.test.Main".to_string(),
        asset_index: AssetIndex {
            id: "5".to_string(),
            url: format!("{server_uri}/assets/index.json"),
            sha1: "0".repeat(40),
        },
        downloads: VersionDownloads {
            client: Download {
                url: format!("{server_uri}/client.jar"),
                sha1: "0".repeat(40),
                size: 0,
            },
        },
        libraries: vec![Library {
            name: "org.slf4j:slf4j-api:2.0.7".to_string(),
            downloads: LibraryDownloads {
                artifact: Some(Artifact {
                    url: format!("{server_uri}/slf4j.jar"),
                    path: "org/slf4j/slf4j-api/2.0.7/slf4j-api-2.0.7.jar".to_string(),
                    sha1: "0".repeat(40),
                    size: 11,
                }),
            },
            rules: None,
        }],
        java_version: Some(JavaVersion { major_version: 17 }),
    }
}

#[tokio::test]
async fn download_libraries_writes_artifact_to_meta_dir() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/slf4j.jar"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"jar-bytes".to_vec()))
        .expect(1)
        .mount(&server)
        .await;

    let meta = meta_with_one_library(&server.uri());
    let tmp = tempfile::tempdir().unwrap();

    download_libraries(&HttpClient::new(), &meta, tmp.path())
        .await
        .expect("download_libraries");

    let written = tmp
        .path()
        .join("cache/minecraft/libraries/org/slf4j/slf4j-api/2.0.7/slf4j-api-2.0.7.jar");
    assert!(written.exists(), "expected library file at {written:?}");
    let contents = std::fs::read(&written).unwrap();
    assert_eq!(contents, b"jar-bytes");
}

#[tokio::test]
async fn download_libraries_skips_when_destination_exists() {
    // pre-create the destination file. download_libraries should detect it
    // and skip the network entirely; wiremock's expect(0) would panic on any
    // request hitting the mock.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/slf4j.jar"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let meta = meta_with_one_library(&server.uri());
    let tmp = tempfile::tempdir().unwrap();
    let existing = tmp
        .path()
        .join("cache/minecraft/libraries/org/slf4j/slf4j-api/2.0.7/slf4j-api-2.0.7.jar");
    std::fs::create_dir_all(existing.parent().unwrap()).unwrap();
    std::fs::write(&existing, b"already there").unwrap();

    download_libraries(&HttpClient::new(), &meta, tmp.path())
        .await
        .expect("noop succeeds");

    // file untouched, mock untouched (server drop will panic if it isn't)
    assert_eq!(std::fs::read(&existing).unwrap(), b"already there");
}

#[tokio::test]
async fn download_assets_from_writes_index_and_assets() {
    // exercises the full path: index fetch + write, then per-asset download
    // from the configurable assets base. uses download_assets_from so the
    // hardcoded ASSETS_BASE_URL stays out of the way.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/assets/index.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "objects": {
                "minecraft/lang/en_us.json": {
                    "hash": "ab1234567890abcdef1234567890abcdef123456",
                    "size": 11
                }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;
    // asset hash starts with "ab" so the per-asset URL is /<base>/ab/<hash>.
    Mock::given(method("GET"))
        .and(path("/cdn/ab/ab1234567890abcdef1234567890abcdef123456"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"asset-bytes".to_vec()))
        .expect(1)
        .mount(&server)
        .await;

    let meta = meta_with_one_library(&server.uri());
    let tmp = tempfile::tempdir().unwrap();
    let cdn_base = format!("{}/cdn", server.uri());

    download_assets_from(&HttpClient::new(), &meta, tmp.path(), &cdn_base)
        .await
        .expect("download_assets_from");

    let index_path = tmp.path().join("cache/minecraft/assets/indexes/5.json");
    assert!(index_path.exists(), "index file missing");
    let asset_path = tmp
        .path()
        .join("cache/minecraft/assets/objects/ab/ab1234567890abcdef1234567890abcdef123456");
    assert!(asset_path.exists(), "asset file missing");
    assert_eq!(std::fs::read(&asset_path).unwrap(), b"asset-bytes");
}

#[tokio::test]
async fn download_assets_writes_index_when_objects_is_empty() {
    // an asset index with empty objects exercises the index fetch + write
    // path without triggering individual asset downloads (which go to the
    // hardcoded ASSETS_BASE_URL and can't be wiremocked here).
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/assets/index.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"objects": {}})))
        .expect(1)
        .mount(&server)
        .await;

    let meta = meta_with_one_library(&server.uri());
    let tmp = tempfile::tempdir().unwrap();

    download_assets(&HttpClient::new(), &meta, tmp.path())
        .await
        .expect("download_assets index-only");

    let index_path = tmp.path().join("cache/minecraft/assets/indexes/5.json");
    assert!(
        index_path.exists(),
        "expected asset index at {index_path:?}"
    );
    let body: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&index_path).unwrap()).unwrap();
    assert!(body.get("objects").is_some());
}
