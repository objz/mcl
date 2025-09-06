use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};
use tracing::Level;

use super::base::PopupFrame;
use crate::tui::error_buffer::{ErrorEvent, AUTO_DISMISS_MS, SLIDE_START_MS};
use crate::tui::theme::THEME;

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
        let (border_color, label) = match self.event.level {
            Level::ERROR => (Color::Red, "ERROR"),
            Level::WARN => (Color::Yellow, "WARN"),
            _ => (THEME.colors.border_focused, "INFO"),
        };

        let title = Line::from(vec![Span::styled(
            format!(" {} ", label),
            Style::default()
                .fg(Color::Black)
                .bg(border_color)
                .add_modifier(Modifier::BOLD),
        )]);

        let message = self.event.message.clone();
        let popup = PopupFrame {
            title,
            border_color,
            bg: None,
            keybinds: None,
            search_line: None,
            content: Box::new(move |inner, buf| {
                Paragraph::new(message.as_str())
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(THEME.colors.foreground))
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

    if elapsed_ms >= AUTO_DISMISS_MS {
        return None;
    }

    let (w, h) = word_wrap_size(message, MAX_W);
    let inner_w = w.max(MIN_W);

    let popup_w = (inner_w + 2) as u16;
    let popup_h = (h + 2) as u16;
    let popup_w = popup_w.min(frame_area.width.saturating_sub(4));
    let popup_h = popup_h.min(frame_area.height.saturating_sub(2));
    let base_x = frame_area.width.saturating_sub(popup_w + 2);

    if elapsed_ms < SLIDE_START_MS {
        return Some(Rect {
            x: base_x,
            y: base_y,
            width: popup_w,
            height: popup_h,
        });
    }

    let progress = (elapsed_ms - SLIDE_START_MS) as f32 / (AUTO_DISMISS_MS - SLIDE_START_MS) as f32;
    let slide = (progress * popup_w as f32) as u16;
    let new_x = base_x.saturating_add(slide);

    if new_x >= frame_area.width {
        return None;
    }

    let visible_w = frame_area.width.saturating_sub(new_x);
    Some(Rect {
        x: new_x,
        y: base_y,
        width: visible_w.min(popup_w),
        height: popup_h,
    })
}
