use super::keybind_lines_wrapped;

#[test]
fn keybind_wrapping_uses_terminal_width_for_unicode_keys() {
    let lines = keybind_lines_wrapped(&[("⏎", " select"), ("a", " add")], 19);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].width(), 19);
}
