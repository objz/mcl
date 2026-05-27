// integration tests for the loader-specific net::* fetchers (forge,
// fabric, quilt, neoforge). each module's fetch_* function now has a
// _from variant that lets the test point the HTTP call at a wiremock
// server; the production constants stay unchanged.

use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use rmcl::net::HttpClient;
use rmcl::net::fabric::{fetch_fabric_game_versions_from, fetch_fabric_versions_from};
use rmcl::net::forge::{fetch_forge_game_versions_from, fetch_forge_versions_from};
use rmcl::net::neoforge::fetch_neoforge_versions_from;
use rmcl::net::quilt::{fetch_quilt_game_versions_from, fetch_quilt_versions_from};

// ---------- forge ----------

#[tokio::test]
async fn forge_fetch_versions_filters_by_prefix() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/promotions.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "promos": {
                "1.20.1-latest": "47.2.0",
                "1.20.1-recommended": "47.1.0",
                "1.19.4-latest": "45.1.0",
                "1.7.10-latest": "10.13.4.1614"
            }
        })))
        .mount(&server)
        .await;

    let url = format!("{}/promotions.json", server.uri());
    let versions = fetch_forge_versions_from(&HttpClient::new(), &url, "1.20.1")
        .await
        .expect("forge versions");

    assert_eq!(versions, vec!["47.1.0", "47.2.0"]);
}

#[tokio::test]
async fn forge_fetch_game_versions_extracts_unique() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/promotions.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "promos": {
                "1.20.1-latest": "47.2.0",
                "1.20.1-recommended": "47.1.0",
                "1.19.4-latest": "45.1.0"
            }
        })))
        .mount(&server)
        .await;

    let url = format!("{}/promotions.json", server.uri());
    let versions = fetch_forge_game_versions_from(&HttpClient::new(), &url)
        .await
        .expect("forge game versions");

    let ids: Vec<&str> = versions.iter().map(|v| v.id.as_str()).collect();
    // dedup + reverse-sort
    assert_eq!(ids, vec!["1.20.1", "1.19.4"]);
}

// ---------- fabric ----------

#[tokio::test]
async fn fabric_fetch_game_versions_parses_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/versions/game"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "version": "1.20.1", "stable": true },
            { "version": "24w01a", "stable": false }
        ])))
        .mount(&server)
        .await;

    let versions = fetch_fabric_game_versions_from(&HttpClient::new(), &server.uri())
        .await
        .expect("fabric game versions");

    assert_eq!(versions.len(), 2);
    assert_eq!(versions[0].id, "1.20.1");
    assert!(versions[0].stable);
    assert_eq!(versions[1].id, "24w01a");
    assert!(!versions[1].stable);
}

#[tokio::test]
async fn fabric_fetch_versions_parses_loader_entries() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/versions/loader/1.20.1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "loader": { "version": "0.15.0", "stable": true },
                "intermediary": { "version": "1.20.1", "stable": true }
            }
        ])))
        .mount(&server)
        .await;

    let versions = fetch_fabric_versions_from(&HttpClient::new(), &server.uri(), "1.20.1")
        .await
        .expect("fabric versions");

    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].loader.version, "0.15.0");
}

// ---------- quilt ----------

#[tokio::test]
async fn quilt_fetch_game_versions_parses_response() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/versions/game"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "version": "1.20.1", "stable": true }
        ])))
        .mount(&server)
        .await;

    let versions = fetch_quilt_game_versions_from(&HttpClient::new(), &server.uri())
        .await
        .expect("quilt game versions");
    assert_eq!(versions[0].id, "1.20.1");
}

#[tokio::test]
async fn quilt_fetch_versions_parses_loader_entries() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/versions/loader/1.20.1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            { "loader": { "version": "0.23.0" } }
        ])))
        .mount(&server)
        .await;

    let versions = fetch_quilt_versions_from(&HttpClient::new(), &server.uri(), "1.20.1")
        .await
        .expect("quilt versions");
    assert_eq!(versions[0].loader.version, "0.23.0");
}

// ---------- neoforge ----------

#[tokio::test]
async fn neoforge_fetch_versions_filters_by_prefix() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/maven-api"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "versions": [
                "20.4.190",
                "20.4.180-beta",
                "20.4.150",
                "21.0.10",
                "21.0.5-alpha"
            ]
        })))
        .mount(&server)
        .await;

    let url = format!("{}/maven-api", server.uri());
    let versions = fetch_neoforge_versions_from(&HttpClient::new(), &url, "1.20.4")
        .await
        .expect("neoforge versions");

    // game version "1.20.4" maps to prefix "20.4." and beta/alpha are excluded
    assert_eq!(versions, vec!["20.4.190", "20.4.150"]);
}

#[tokio::test]
async fn neoforge_fetch_versions_rejects_invalid_game_version() {
    let server = MockServer::start().await;
    let url = format!("{}/maven-api", server.uri());
    // no mock - the function should error out before hitting the network
    let err = fetch_neoforge_versions_from(&HttpClient::new(), &url, "bogus")
        .await
        .expect_err("invalid game version");
    assert!(format!("{err:?}").contains("Invalid game version"));
}
