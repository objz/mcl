use super::*;

#[test]
fn mode_labels_have_the_same_rendered_width() {
    assert_eq!(
        mode_label(ContentMode::Installed).chars().count(),
        mode_label(ContentMode::Discover).chars().count()
    );
}

#[test]
fn discovery_navigation_only_cycles_downloadable_tabs() {
    assert_eq!(
        ContentTab::Shaders.next_for_mode(ContentMode::Discover),
        ContentTab::Mods
    );
    assert_eq!(
        ContentTab::Mods.previous_for_mode(ContentMode::Discover),
        ContentTab::Shaders
    );
}

#[test]
fn discovery_navigation_recovers_from_hidden_local_tab() {
    assert_eq!(
        ContentTab::Logs.next_for_mode(ContentMode::Discover),
        ContentTab::ResourcePacks
    );
}

#[test]
fn discovery_version_rows_only_show_the_version_number() {
    let version = crate::net::modrinth::VersionInfo {
        id: "version-id".to_owned(),
        project_id: "project-id".to_owned(),
        name: "A descriptive release title".to_owned(),
        version_number: "3.2.4-fabric-26.1".to_owned(),
        game_versions: vec![],
        loaders: vec![],
        date_published: String::new(),
        files: vec![],
    };

    assert_eq!(discovery_version_label(&version), "3.2.4-fabric-26.1");
}

#[test]
fn discovery_version_popup_uses_compact_heights() {
    assert_eq!(version_popup_height(1, false, false, false), 6);
    assert_eq!(version_popup_height(100, false, false, false), 18);
    assert_eq!(version_popup_height(1, true, false, false), 8);
    assert_eq!(version_popup_height(0, false, true, false), 5);
    assert_eq!(version_popup_height(0, false, false, true), 8);
}

#[test]
fn confirmation_metadata_is_human_readable() {
    assert_eq!(
        confirmation_loaders(&["fabric".to_owned(), "neoforge".to_owned()]),
        "Fabric, NeoForge"
    );
    assert_eq!(
        confirmation_release_date("2026-07-26T14:30:00Z"),
        "2026-07-26"
    );
    assert_eq!(confirmation_values(&[]), "Unknown");
}
