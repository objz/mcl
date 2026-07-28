use super::*;

#[test]
fn curseforge_file_maps_to_shared_version() {
    let file: File = serde_json::from_str(
        r#"{
            "id": 9,
            "modId": 7,
            "displayName": "Example 1.0",
            "fileName": "example.jar",
            "fileLength": 12,
            "downloadUrl": "https://example.invalid/example.jar",
            "gameVersions": ["1.21.1", "Fabric"],
            "releaseType": 2,
            "dependencies": [
                {"modId": 8, "relationType": 3},
                {"modId": 9, "relationType": 2},
                {"modId": 10, "relationType": 5}
            ],
            "hashes": [{"value": "abc", "algo": 1}]
        }"#,
    )
    .unwrap();
    let version = version_info(file);
    assert_eq!(version.project_id, "7");
    assert_eq!(version.loaders, ["fabric"]);
    assert_eq!(version.version_type, VersionType::Beta);
    assert_eq!(
        version
            .dependencies
            .iter()
            .map(|dependency| dependency.dependency_type)
            .collect::<Vec<_>>(),
        [
            DependencyType::Required,
            DependencyType::Optional,
            DependencyType::Incompatible
        ]
    );
    assert_eq!(version.files[0].hashes["sha1"], "abc");
}

#[test]
fn curseforge_library_category_maps_to_cleanup_metadata() {
    let project: Mod = serde_json::from_str(
        r#"{
            "id": 7,
            "name": "Library",
            "slug": "library",
            "categories": [{
                "name": "API and Library",
                "slug": "library-api"
            }]
        }"#,
    )
    .unwrap();

    assert!(project_info(project, String::new()).is_library_only());
}

#[tokio::test]
async fn curseforge_versions_follow_pagination() {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    let files = |range: std::ops::Range<u64>| {
        range
            .map(|id| {
                serde_json::json!({
                    "id": id,
                    "modId": 7,
                    "displayName": format!("Version {id}"),
                    "fileName": format!("{id}.jar")
                })
            })
            .collect::<Vec<_>>()
    };
    Mock::given(method("GET"))
        .and(path("/mods/7/files"))
        .and(query_param("index", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": files(0..50),
            "pagination": {"totalCount": 51}
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/mods/7/files"))
        .and(query_param("index", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": files(50..51),
            "pagination": {"totalCount": 51}
        })))
        .mount(&server)
        .await;

    let versions =
        fetch_versions_from(&HttpClient::new(), "test-key", &server.uri(), "7", "", None)
            .await
            .unwrap();
    assert_eq!(versions.len(), 51);
}
