use super::*;

#[test]
fn minecraft_events_are_not_general_app_logs() {
    assert!(!should_record_app_log(MINECRAFT_LOG_TARGET, Level::ERROR));
}

#[test]
fn rmcl_events_are_general_app_logs() {
    assert!(should_record_app_log(
        "rmcl::instance::manager",
        Level::TRACE
    ));
}

#[test]
fn dependency_debug_events_are_not_general_app_logs() {
    assert!(!should_record_app_log("log", Level::DEBUG));
}

#[test]
fn dependency_warnings_are_general_app_logs() {
    assert!(should_record_app_log("notify", Level::WARN));
}

#[test]
fn svg_renderer_warnings_do_not_interrupt_the_ui() {
    assert!(!should_record_app_log("usvg::text", Level::WARN));
    assert!(!should_record_app_log("usvg::parser::filter", Level::WARN));
}

#[test]
fn unwritable_log_path_falls_back_without_panicking() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("not-a-directory");
    std::fs::write(&file, b"block nested directory creation").unwrap();

    let (_writer, _guard) = open_log_writer(&file.join("rmcl"));
}
