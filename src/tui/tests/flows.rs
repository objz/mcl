use crossterm::event::{KeyCode, MouseEventKind};

use super::harness::UiHarness;
use crate::instance::ProviderProject;
use crate::tui::{
    app::{FocusedArea, ProviderConflictState},
    widgets::{
        content::{ContentMode, ContentTab},
        popups::confirm,
    },
};

#[test]
fn global_navigation_returns_from_log_overlay() {
    let mut ui = UiHarness::new();

    ui.key(KeyCode::Char('C'));
    assert_eq!(ui.app.focused, FocusedArea::Content);

    ui.key(KeyCode::Char('O'));
    assert_eq!(ui.app.focused, FocusedArea::OverviewExpanded);

    ui.key(KeyCode::Esc);
    assert_eq!(ui.app.focused, FocusedArea::Content);

    ui.key(KeyCode::Char('q'));
    assert!(ui.app.exit);
}

#[test]
fn mouse_wheel_uses_the_focused_views_scroll_navigation() {
    let mut ui = UiHarness::new();
    ui.app.focused = FocusedArea::OverviewExpanded;
    ui.app.log_overlay_max_scroll = 1;

    ui.mouse(MouseEventKind::ScrollDown);
    ui.mouse(MouseEventKind::ScrollDown);
    assert_eq!(ui.app.log_overlay_scroll, 1);

    ui.mouse(MouseEventKind::ScrollUp);
    assert_eq!(ui.app.log_overlay_scroll, 0);
}

#[test]
fn discovery_mode_recovers_from_a_hidden_tab_and_cycles_visible_tabs() {
    let mut ui = UiHarness::new();
    ui.app.focused = FocusedArea::Content;
    ui.app.content_tab = ContentTab::Logs;

    ui.key(KeyCode::Tab);
    assert_eq!(ui.app.content_mode, ContentMode::Discover);
    assert_eq!(ui.app.content_tab, ContentTab::Mods);

    ui.key(KeyCode::Right);
    assert_eq!(ui.app.content_tab, ContentTab::ResourcePacks);

    ui.key(KeyCode::Left);
    assert_eq!(ui.app.content_tab, ContentTab::Mods);
}

#[test]
fn instance_delete_can_be_cancelled_without_touching_disk() {
    let mut ui = UiHarness::new();
    ui.add_instance("Test Instance");
    let instance_path = ui.instance_path("Test Instance");

    ui.key(KeyCode::Char('d'));
    assert_eq!(ui.app.focused, FocusedArea::ConfirmDelete);
    assert!(matches!(
        confirm::pending_target(),
        Some(confirm::ConfirmTarget::Instance { name }) if name == "Test Instance"
    ));

    ui.key(KeyCode::Esc);
    assert_eq!(ui.app.focused, FocusedArea::Instances);
    assert!(confirm::pending_target().is_none());
    assert_eq!(ui.app.instances_state.instances.len(), 1);
    assert!(instance_path.exists());
}

#[test]
fn confirmed_instance_delete_removes_state_and_disk() {
    let mut ui = UiHarness::new();
    let name = format!("rmcl-ui-test-{}", std::process::id());
    ui.add_instance(&name);
    let instance_path = ui.instance_path(&name);
    assert!(!crate::instance::desktop::exists(&name));

    ui.key(KeyCode::Char('d'));
    ui.draw();
    assert!(ui.screen().contains(&format!("Delete '{name}'")));
    assert!(
        ui.screen()
            .contains("This will permanently remove the instance")
    );
    ui.key(KeyCode::Char('y'));

    assert_eq!(ui.app.focused, FocusedArea::Instances);
    assert!(ui.app.instances_state.instances.is_empty());
    assert!(!instance_path.exists());
}

#[test]
fn confirmed_screenshot_delete_removes_state_and_file() {
    let mut ui = UiHarness::new();
    ui.add_instance("Screenshots");
    let path = ui
        .instance_path("Screenshots")
        .join(crate::storage::MINECRAFT_DIR_NAME)
        .join("screenshots")
        .join("shot.png");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"image").unwrap();
    ui.app.screenshots_state.entries = vec![crate::instance::screenshots::ScreenshotEntry {
        name: "shot.png".to_owned(),
        path: path.clone(),
        width: 1,
        height: 1,
    }];
    ui.app.focused = FocusedArea::Content;
    ui.app.content_tab = ContentTab::Screenshots;

    ui.key(KeyCode::Char('d'));
    ui.key(KeyCode::Enter);

    assert_eq!(ui.app.focused, FocusedArea::Content);
    assert!(ui.app.screenshots_state.entries.is_empty());
    assert!(!path.exists());
}

#[test]
fn confirmed_account_delete_updates_the_account_panel() {
    let mut ui = UiHarness::new();
    ui.add_account("Player");
    ui.app.focused = FocusedArea::Account;

    ui.key(KeyCode::Char('d'));
    ui.key(KeyCode::Char('y'));

    assert_eq!(ui.app.focused, FocusedArea::Account);
    assert!(ui.app.account_state.store.accounts.is_empty());
    assert_eq!(ui.app.account_state.list_state.selected, None);
}

#[test]
fn settings_profile_can_be_created_from_key_events() {
    let mut ui = UiHarness::new();
    ui.app.focused = FocusedArea::Settings;

    ui.key(KeyCode::Char('a'));
    for character in "shared".chars() {
        ui.key(KeyCode::Char(character));
    }
    ui.key(KeyCode::Enter);

    assert_eq!(ui.app.settings_state.profiles, ["shared"]);
}

#[test]
fn instance_wizards_open_render_and_cancel_through_app_input() {
    let mut ui = UiHarness::new();

    ui.key(KeyCode::Char('a'));
    assert_eq!(ui.app.focused, FocusedArea::Popup);
    ui.draw();
    assert!(ui.screen().contains("New Instance"));
    ui.key(KeyCode::Esc);
    assert_eq!(ui.app.focused, FocusedArea::Instances);

    ui.key(KeyCode::Char('m'));
    assert_eq!(ui.app.focused, FocusedArea::ImportPopup);
    ui.draw();
    assert!(ui.screen().contains("Browse Modpacks"));
    ui.key(KeyCode::Char('d'));
    ui.draw();
    assert!(ui.screen().contains("Direct Modpack Import"));
    ui.key(KeyCode::Esc);
    assert_eq!(ui.app.focused, FocusedArea::ImportPopup);
    ui.key(KeyCode::Esc);
    assert_eq!(ui.app.focused, FocusedArea::Instances);
}

#[test]
fn provider_conflict_renders_and_can_be_deferred() {
    let mut ui = UiHarness::new();
    ui.app.provider_conflict = Some(ProviderConflictState {
        relative_path: "mods/example.jar".into(),
        candidates: vec![
            ProviderProject {
                provider: "modrinth".to_owned(),
                project_id: "first".to_owned(),
                version_id: "1".to_owned(),
            },
            ProviderProject {
                provider: "curseforge".to_owned(),
                project_id: "second".to_owned(),
                version_id: "2".to_owned(),
            },
        ],
        selected: 0,
    });

    ui.draw();
    assert!(ui.screen().contains("Choose provider for example.jar"));

    ui.key(KeyCode::Down);
    assert_eq!(ui.app.provider_conflict.as_ref().unwrap().selected, 1);
    ui.key(KeyCode::Esc);

    assert!(ui.app.provider_conflict.is_none());
    assert!(
        ui.app
            .dismissed_provider_conflicts
            .contains(std::path::Path::new("mods/example.jar"))
    );
}

#[test]
fn provider_conflict_selection_is_persisted() {
    let mut ui = UiHarness::new();
    ui.add_instance("Conflict");
    let relative_path = std::path::PathBuf::from("mods/example.jar");
    let candidates = vec![
        ProviderProject {
            provider: "modrinth".to_owned(),
            project_id: "first".to_owned(),
            version_id: "1".to_owned(),
        },
        ProviderProject {
            provider: "curseforge".to_owned(),
            project_id: "second".to_owned(),
            version_id: "2".to_owned(),
        },
    ];
    let manifest = crate::instance::ContentManifest {
        version: 1,
        files: vec![crate::instance::ContentFileRecord {
            relative_path: relative_path.clone(),
            kind: crate::instance::ContentKind::Mod,
            enabled: true,
            fingerprint: crate::instance::FileFingerprint {
                size: 1,
                modified_ns: 0,
                hashes: Default::default(),
            },
            resolution: crate::instance::Resolution::Ambiguous {
                candidates: candidates.clone(),
            },
        }],
    };
    let manifest_path =
        crate::storage::InstancePaths::new(ui.instance_path("Conflict")).content_manifest();
    manifest.save(&manifest_path).unwrap();
    ui.app.content_manifest = Some(("Conflict".to_owned(), manifest));
    ui.app.provider_conflict = Some(ProviderConflictState {
        relative_path: relative_path.clone(),
        candidates,
        selected: 0,
    });

    ui.key(KeyCode::Down);
    ui.key(KeyCode::Enter);

    assert!(ui.app.provider_conflict.is_none());
    let saved = crate::instance::ContentManifest::load(&manifest_path).unwrap();
    assert!(matches!(
        &saved.record(&relative_path).unwrap().resolution,
        crate::instance::Resolution::Resolved { project }
            if project.provider == "curseforge" && project.project_id == "second"
    ));
}
