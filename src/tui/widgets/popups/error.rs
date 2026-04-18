use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};
use tracing::Level;

use super::base::PopupFrame;
use crate::config::SETTINGS;
use crate::tui::error_buffer::ErrorEvent;
use crate::config::theme::THEME;

pub struct ErrorPopup {
    pub event: ErrorEvent,
}

impl ErrorPopup {
    pub fn new(event: ErrorEvent) -> Self {
        Self { event }
    }
}

impl Widget for ErrorPopup {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let theme = THEME.as_ref();
        let (border_color, label) = match self.event.level {
            Level::ERROR => (theme.error(), "ERROR"),
            Level::WARN => (theme.warning(), "WARN"),
            _ => (theme.text_dim(), "INFO"),
        };

        let title = Line::from(vec![Span::styled(
            format!(" {} ", label),
            Style::default()
                .fg(theme.text_bright())
                .bg(border_color)
                .add_modifier(Modifier::BOLD),
        )]);

        let message = self.event.message.clone();
        let text_color = theme.text();
        let bg_color = theme.surface();
        let popup = PopupFrame {
            title,
            border_color,
            bg: Some(bg_color),
            keybinds: None,
            search_line: None,
            content: Box::new(move |inner, buf| {
                Paragraph::new(message.as_str())
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(text_color))
                    .render(inner, buf);
            }),
        };

        popup.render(area, buf);
    }
}

pub fn popup_area(frame_area: Rect, message: &str, base_y: u16, elapsed_ms: u128) -> Option<Rect> {
    use super::word_wrap_size;

    const MAX_W: usize = 58;
    const MIN_W: usize = 22;

    if elapsed_ms >= SETTINGS.ui.error_auto_dismiss_ms as u128 {
        return None;
    }

    let (w, h) = word_wrap_size(message, MAX_W);
    let inner_w = w.max(MIN_W);

    let popup_w = (inner_w + 2) as u16;
    let popup_h = (h + 2) as u16;
    let popup_w = popup_w.min(frame_area.width.saturating_sub(4));
    let max_h = frame_area.height.saturating_sub(base_y).saturating_sub(1);
    if max_h < 3 {
        return None;
    }
    let popup_h = popup_h.min(max_h);
    let base_x = frame_area.width.saturating_sub(popup_w + 2);
    Some(Rect {
        x: base_x,
        y: base_y,
        width: popup_w,
        height: popup_h,
    })
}
