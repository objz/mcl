use super::harness::UiHarness;

#[test]
fn empty_app_renders_the_complete_frame() {
    let mut ui = UiHarness::new();
    ui.draw();
    insta::assert_snapshot!(ui.screen());
}
