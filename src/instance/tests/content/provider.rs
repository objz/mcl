use super::*;

#[test]
fn registry_falls_back_to_first_capable_provider() {
    let registry = ProviderRegistry::modrinth(crate::net::HttpClient::new());
    assert_eq!(registry.preferred("unknown").unwrap().id(), "modrinth");
}

#[test]
fn update_check_requires_the_installed_version_behind_a_newer_result() {
    let version = |id: &str| VersionInfo {
        id: id.to_owned(),
        project_id: "project".to_owned(),
        name: id.to_owned(),
        version_number: id.to_owned(),
        game_versions: vec!["1.21.1".to_owned()],
        loaders: vec!["fabric".to_owned()],
        version_type: crate::net::modrinth::VersionType::Release,
        dependencies: Vec::new(),
        date_published: String::new(),
        files: Vec::new(),
    };
    let versions = vec![version("new"), version("installed")];

    assert!(has_newer_compatible_version(&versions, "installed"));
    assert!(!has_newer_compatible_version(&versions, "new"));
    assert!(!has_newer_compatible_version(&versions, "missing"));
}
