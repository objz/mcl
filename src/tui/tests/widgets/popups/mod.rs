use super::{
    confirm::{ConfirmTarget, confirm_popup_area},
    keybind_lines_wrapped, word_wrap_size,
};

#[test]
fn keybind_wrapping_uses_terminal_width_for_unicode_keys() {
    let lines = keybind_lines_wrapped(&[("⏎", " select"), ("a", " add")], 19);

    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].width(), 19);
}

#[test]
fn word_wrap_size_uses_terminal_width_for_unicode_text() {
    assert_eq!(word_wrap_size("Übergröße", 20), (9, 1));
    assert_eq!(word_wrap_size("世界 test", 6), (4, 2));
    assert_eq!(word_wrap_size("one\n\ntwo", 20), (3, 3));
}

#[test]
fn confirmation_area_uses_display_width_for_unicode_names() {
    let frame = ratatui::layout::Rect::new(0, 0, 100, 30);
    let ascii = ConfirmTarget::Instance {
        name: "a".repeat(30),
    };
    let unicode = ConfirmTarget::Instance {
        name: format!("é{}", "a".repeat(29)),
    };

    assert_eq!(
        confirm_popup_area(frame, &ascii),
        confirm_popup_area(frame, &unicode)
    );
}
