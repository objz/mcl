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
