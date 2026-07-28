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
        version_type: crate::net::modrinth::VersionType::Release,
        dependencies: Vec::new(),
        date_published: String::new(),
        files: vec![],
    };

    assert_eq!(discovery_version_label(&version), "3.2.4-fabric-26.1");
}

#[test]
fn discovery_confirmation_popup_fits_its_summary() {
    assert_eq!(version_popup_height(false, None), VERSION_POPUP_HEIGHT);
    assert_eq!(version_popup_height(true, None), 6);
}

#[test]
fn discovery_version_popup_renders_over_a_project_page() {
    use crate::instance::ContentKind;
    use crate::net::modrinth::DiscoveryProject;
    use ratatui::{Terminal, backend::TestBackend};

    let project = DiscoveryProject {
        id: "project".to_owned(),
        slug: "project".to_owned(),
        title: "Project".to_owned(),
        description: String::new(),
        downloads: 0,
        icon_url: None,
        icon_bytes: None,
    };
    let mut state = DiscoveryState::new(ContentKind::Mod);
    state
        .list
        .entries
        .push(crate::tui::widgets::content::discovery::project_entry(
            project, None,
        ));
    state.list.list_state.selected = Some(0);
    state.begin_project_page();
    state.begin_versions();

    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let picker = ratatui_image::picker::Picker::halfblocks();
    terminal
        .draw(|frame| render_discovery_popup(frame, frame.area(), &mut state, &picker))
        .unwrap();

    assert!(format!("{}", terminal.backend()).contains("Install Project"));
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
