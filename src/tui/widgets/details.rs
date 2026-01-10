use ratatui::{
    layout::{Alignment, Rect},
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

    let block = Block::default()
        .title(styled_title("Details", true))
        .title_bottom(
            super::popups::keybind_line(&[
                ("P", " profiles"),
                ("C", " content"),
                ("A", " account"),
                ("S", " logs"),
                ("q", " quit"),
            ])
            .alignment(Alignment::Right),
        )
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color));

    frame.render_widget(block, area);
}
