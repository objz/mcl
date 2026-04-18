// "are you sure?" popup for instance deletion. uses global state so the
// confirmation target persists across render frames.

use std::sync::LazyLock;
use std::sync::Mutex;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use crate::config::theme::THEME;

static CONFIRM_STATE: LazyLock<Mutex<ConfirmState>> = LazyLock::new(|| Mutex::new(ConfirmState::default()));

#[derive(Debug, Default)]
struct ConfirmState {
    instance_name: String,
}

pub fn set_pending_delete(name: impl Into<String>) {
    match CONFIRM_STATE.lock() {
        Ok(mut s) => {
            s.instance_name = name.into();
        }
        Err(e) => {
            tracing::error!("Confirm popup state lock poisoned: {}", e);
        }
    }
}

pub fn pending_delete_name() -> String {
    match CONFIRM_STATE.lock() {
        Ok(s) => s.instance_name.clone(),
        Err(_) => String::new(),
    }
}

pub fn clear_pending() {
    match CONFIRM_STATE.lock() {
        Ok(mut s) => {
            s.instance_name.clear();
        }
        Err(e) => {
            tracing::error!("Confirm popup state lock poisoned: {}", e);
        }
    }
}

pub struct ConfirmPopup {
    instance_name: String,
}

impl ConfirmPopup {
    pub fn new(instance_name: impl Into<String>) -> Self {
        Self {
            instance_name: instance_name.into(),
        }
    }
}

impl Widget for ConfirmPopup {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use super::{base::PopupFrame, keybind_line};

        let theme = THEME.as_ref();
        let title = Line::from(vec![Span::styled(
            format!(" Delete '{}' ", self.instance_name),
            Style::default()
                .fg(theme.text_dim())
                .add_modifier(Modifier::BOLD),
        )]);
        let kb = keybind_line(&[("Enter", " confirm")]);

        let border_color = theme.text_dim();
        let bg_color = theme.surface();
        let text_color = theme.text();
        let popup = PopupFrame {
            title,
            border_color,
            bg: Some(bg_color),
            keybinds: Some(kb),
            search_line: None,
            content: Box::new(move |inner, buf| {
                Paragraph::new("This will permanently remove the instance")
                    .style(Style::default().fg(text_color))
                    .render(inner, buf);
            }),
        };

        popup.render(area, buf);
    }
}

pub fn confirm_popup_area(frame_area: Rect, name: &str) -> Rect {
    use super::word_wrap_size;
    use ratatui::layout::Constraint;
    const MAX_W: usize = 48;
    const BODY: &str = "This will permanently remove the instance";
    let title_w = name.len() + 12;
    let (body_w, _) = word_wrap_size(BODY, MAX_W);
    let inner_w = title_w.max(body_w).min(MAX_W);
    let (_, lines) = word_wrap_size(BODY, inner_w);
    let popup_w = ((inner_w + 2) as u16).min(frame_area.width.saturating_sub(4));
    let popup_h = ((lines + 2) as u16).min(frame_area.height.saturating_sub(4));
    frame_area.centered(Constraint::Length(popup_w), Constraint::Length(popup_h))
}
