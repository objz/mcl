// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use std::num::NonZeroU16;

use super::*;

#[test]
fn popup_cells_covering_terminal_images_are_always_redrawn() {
    let area = Rect::new(0, 0, 4, 3);
    let mut buffer = Buffer::empty(area);
    buffer[(1, 1)].set_diff_option(CellDiffOption::Skip);
    buffer[(2, 1)]
        .set_symbol("\x1b_Gimage\x1b\\")
        .set_diff_option(CellDiffOption::ForcedWidth(NonZeroU16::new(1).unwrap()));
    let popup = PopupFrame {
        title: Line::from("Popup"),
        border_color: Color::White,
        bg: Some(Color::Black),
        keybinds: None,
        search_line: None,
        content: Box::new(|_, _| {}),
    };

    popup.render(area, &mut buffer);

    assert_eq!(buffer[(1, 1)].diff_option, CellDiffOption::AlwaysUpdate);
    assert_eq!(buffer[(2, 1)].diff_option, CellDiffOption::AlwaysUpdate);
    assert_eq!(buffer[(0, 1)].diff_option, CellDiffOption::None);
}
