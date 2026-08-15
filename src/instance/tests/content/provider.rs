use super::*;

#[test]
fn registry_falls_back_to_first_capable_provider() {
    let registry = ProviderRegistry::modrinth(crate::net::HttpClient::new());
    assert_eq!(registry.preferred("unknown").unwrap().id(), "modrinth");
}

fn version(id: &str, date_published: &str) -> VersionInfo {
    VersionInfo {
        id: id.to_owned(),
        project_id: "project".to_owned(),
        name: id.to_owned(),
        version_number: id.to_owned(),
        game_versions: vec!["1.21.1".to_owned()],
        loaders: vec!["fabric".to_owned()],
        version_type: crate::net::modrinth::VersionType::Release,
        dependencies: Vec::new(),
        date_published: date_published.to_owned(),
        files: Vec::new(),
    }
}

#[test]
fn update_check_requires_the_installed_version_behind_a_newer_result() {
    let versions = vec![version("new", ""), version("installed", "")];

    assert!(has_newer_compatible_version(&versions, "installed"));
    assert!(!has_newer_compatible_version(&versions, "new"));
    assert!(!has_newer_compatible_version(&versions, "missing"));
}

#[test]
fn update_check_uses_publish_dates_not_response_order() {
    // curseforge does not promise a newest-first ordering
    let versions = vec![
        version("installed", "2024-01-01T00:00:00Z"),
        version("new", "2024-06-01T00:00:00.500Z"),
    ];

    assert_eq!(
        newest_version(&versions).map(|v| v.id.as_str()),
        Some("new")
    );
    assert!(has_newer_compatible_version(&versions, "installed"));
    assert!(!has_newer_compatible_version(&versions, "new"));
}

#[test]
fn a_version_missing_from_the_compatible_list_can_still_be_outdated() {
    // a modpack pins files that are not tagged for the instance's exact game
    // version, so the installed version never shows up in the filtered list
    let compatible = vec![version("new", "2024-06-01T00:00:00Z")];
    let pinned = version("pinned", "2023-01-01T00:00:00Z");

    assert!(is_newer(newest_version(&compatible).unwrap(), &pinned));
    assert!(!has_newer_compatible_version(&compatible, &pinned.id));
}
