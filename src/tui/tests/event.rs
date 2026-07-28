use super::*;
use crate::tui::tests::harness::UiHarness;

#[test]
fn edited_instance_config_reloads_into_the_ui() {
    let mut ui = UiHarness::new();
    ui.add_instance("Edited");
    let mut config = ui.app.instances_state.selected_instance().unwrap().clone();
    config.memory_max = Some("8G".to_owned());
    ui.app.instance_manager.save(&config).unwrap();

    ui.app
        .reload_edited_config(&ui.instance_path("Edited").join("instance.json"));

    assert_eq!(
        ui.app
            .instances_state
            .selected_instance()
            .unwrap()
            .memory_max
            .as_deref(),
        Some("8G")
    );
}

#[test]
fn completed_background_instance_is_drained_into_the_ui() {
    let mut ui = UiHarness::new();
    ui.add_instance("Existing");
    ui.add_instance("Pending");
    let config = ui.app.instances_state.instances.pop().unwrap();
    ui.app.instances_state.list_state.selected = Some(0);
    PENDING_INSTANCES.lock().unwrap().push(config);

    ui.app.drain_pending_instances();

    assert_eq!(
        ui.app.instances_state.selected_instance().unwrap().name,
        "Pending"
    );
    assert!(PENDING_INSTANCES.lock().unwrap().is_empty());

    ui.draw();
    assert_eq!(ui.app.mods_state.loaded_for.as_deref(), Some("Pending"));
}

#[test]
fn editor_kind_is_detected_from_the_executable_name() {
    assert!(editor_runs_in_terminal("/usr/bin/nvim"));
    assert!(editor_runs_in_terminal("nano"));
    assert!(!editor_runs_in_terminal("/usr/bin/code"));
}

#[test]
fn overlay_count_tracks_independent_popup_layers() {
    let mut ui = UiHarness::new();
    assert_eq!(ui.app.overlay_count(), 0);

    ui.app.instances_state.show_import_popup = true;
    ui.app.account_state.add_mode = widgets::account::AddMode::ChooseType;
    assert_eq!(ui.app.overlay_count(), 2);

    ui.app.account_state.add_mode = widgets::account::AddMode::None;
    assert_eq!(ui.app.overlay_count(), 1);
}

#[test]
fn terminal_image_markers_exclude_normal_text_and_toggle() {
    use std::num::NonZeroU16;

    let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(4, 7, 3, 1));
    buffer[(4, 7)]
        .set_symbol("\x1b_Gimage\x1b\\")
        .set_diff_option(ratatui::buffer::CellDiffOption::ForcedWidth(
            NonZeroU16::new(1).unwrap(),
        ));
    buffer[(5, 7)].set_symbol("text").set_diff_option(
        ratatui::buffer::CellDiffOption::ForcedWidth(NonZeroU16::new(1).unwrap()),
    );
    buffer[(6, 7)].set_symbol("\x1b[0m");

    mark_terminal_images(&mut buffer, false);
    assert_eq!(buffer[(4, 7)].symbol(), "\x1b_Gimage\x1b\\\u{200b}");
    assert_eq!(buffer[(5, 7)].symbol(), "text");
    assert_eq!(buffer[(6, 7)].symbol(), "\x1b[0m");

    buffer[(4, 7)].set_symbol("\x1b_Gimage\x1b\\");
    mark_terminal_images(&mut buffer, true);
    assert_eq!(buffer[(4, 7)].symbol(), "\x1b_Gimage\x1b\\\u{200b}\u{200b}");
}

#[test]
fn terminal_image_cells_change_when_an_overlay_opens_or_closes() {
    assert!(terminal_image_cells_changed(
        &[true, false, false],
        &[true, true, false]
    ));
    assert!(terminal_image_cells_changed(
        &[true, true, false],
        &[true, false, false]
    ));
    assert!(!terminal_image_cells_changed(&[], &[true]));
    assert!(!terminal_image_cells_changed(
        &[true, false, true],
        &[true, false, true],
    ));
}
