use super::harness::UiHarness;
use crate::instance::ProviderProject;
use crate::tui::app::ProviderConflictState;
use crossterm::event::KeyCode;

#[test]
fn empty_app_renders_the_complete_frame() {
    let mut ui = UiHarness::new();
    ui.draw();
    insta::assert_snapshot!(ui.screen());
}

#[test]
fn active_progress_is_visible_in_the_complete_frame() {
    let mut ui = UiHarness::new();
    crate::tui::progress::set_action("Downloading test data");
    crate::tui::progress::set_sub_action("one.jar");
    crate::tui::progress::set_progress(1, 2);

    ui.draw();

    let screen = ui.screen();
    assert!(screen.contains("Downloading test data"));
    assert!(screen.contains("one.jar"));
    crate::tui::progress::clear();
}

#[test]
fn instance_delete_confirmation_renders_the_complete_frame() {
    let mut ui = UiHarness::new();
    ui.add_instance("Snapshot Instance");
    ui.key(KeyCode::Char('d'));

    ui.draw();

    insta::assert_snapshot!(ui.screen());
}

#[test]
fn provider_conflict_renders_the_complete_frame() {
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

    insta::assert_snapshot!(ui.screen());
}
