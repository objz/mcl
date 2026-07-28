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
    ui.add_instance("Pending");
    let config = ui.app.instances_state.instances.pop().unwrap();
    PENDING_INSTANCES.lock().unwrap().push(config);

    ui.app.drain_pending_instances();

    assert_eq!(
        ui.app.instances_state.selected_instance().unwrap().name,
        "Pending"
    );
    assert!(PENDING_INSTANCES.lock().unwrap().is_empty());
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
fn terminal_image_cells_exclude_normal_text() {
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

    let cells = terminal_image_cells(&buffer);

    assert_eq!(cells.len(), 1);
    assert_eq!((cells[0].0, cells[0].1), (4, 7));
}

#[test]
fn image_cells_are_reexposed_when_an_overlay_shrinks() {
    assert!(image_cells_reexposed(
        &[true, false, false],
        &[true, true, false]
    ));
    assert!(!image_cells_reexposed(
        &[true, true, false],
        &[true, false, false]
    ));
    assert!(!image_cells_reexposed(&[], &[true]));
}
