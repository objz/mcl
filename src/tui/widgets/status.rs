use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};
use tracing::Level;

use crate::tui::layout::FocusedArea;
use crate::tui::logging::STATUS_EVENTS;

use super::styled_title;

pub fn render(frame: &mut Frame, area: Rect, focused: FocusedArea) {
    let border_color = if focused == FocusedArea::Status {
        Color::White
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .title(styled_title("Status", true))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));

    let latest = STATUS_EVENTS
        .lock()
        .ok()
        .and_then(|events| events.back().cloned());

    let content = match latest {
        Some(event) => {
            let (label, label_color) = match event.level {
                Level::ERROR => (" ERROR ", Color::Red),
                Level::WARN => (" WARN ", Color::Yellow),
                _ => (" INFO ", Color::White),
            };

            Paragraph::new(Line::from(vec![
                Span::styled(label, Style::default().fg(Color::White).bg(label_color)),
                Span::raw(" "),
                Span::raw(event.message),
            ]))
        }
        None => Paragraph::new(Span::styled("Ready", Style::default().fg(Color::DarkGray))),
    };

    frame.render_widget(content.block(block), area);
}
