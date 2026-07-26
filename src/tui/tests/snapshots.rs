use super::harness::UiHarness;

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
