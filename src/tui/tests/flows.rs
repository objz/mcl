use crossterm::event::KeyCode;

use super::harness::UiHarness;
use crate::tui::{
    app::FocusedArea,
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
}
