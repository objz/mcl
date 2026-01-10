use ratatui::{
    layout::Rect,
    style::Style,
    widgets::{Block, BorderType, Borders},
    Frame,
};

use crate::tui::layout::FocusedArea;
use crate::tui::theme::THEME;

use super::styled_title;

pub fn render(frame: &mut Frame, area: Rect, focused: FocusedArea) {
    let color = if focused == FocusedArea::Details {
        THEME.colors.border_focused
    } else {
        THEME.colors.border_unfocused
    };

    let mut block = Block::default()
        .title(styled_title("Details", true))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color));

    let lines = super::popups::keybind_lines_wrapped(
        &[
            ("P", " profiles"),
            ("C", " content"),
            ("A", " account"),
            ("S", " logs"),
            ("q", " quit"),
        ],
        area.width.saturating_sub(2),
    );
    for line in lines {
        block = block.title_bottom(line);
    }

    frame.render_widget(block, area);
}
