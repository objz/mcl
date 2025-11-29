use crossterm::event::KeyEvent;
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use crate::tui::theme::THEME;

pub mod account;
pub mod content;
pub mod content_list;
pub mod details;
pub mod logs_viewer;
pub mod screenshots_grid;
pub mod popups;
pub mod profiles;
pub mod search;
pub mod status;

pub fn styled_title(title: &str, highlight: bool) -> Line<'_> {
    if !highlight || title.is_empty() {
        Line::from(Span::raw(title))
    } else {
        let mut chars = title.chars();
        let first = chars.next().unwrap_or_default().to_string();
        let rest: String = chars.collect();
        Line::from(vec![
            Span::styled(first, Style::default().fg(THEME.colors.accent)),
            Span::raw(rest),
        ])
    }
}

pub trait WidgetKey {
    fn handle_key(&mut self, key_event: &KeyEvent);
}
