use crate::config::theme::THEME;
use crossterm::event::KeyEvent;
use ratatui::{
    style::Style,
    text::{Line, Span},
};

pub mod account;
pub mod content;
pub mod details;
pub mod logs_viewer;
pub mod popups;
pub mod instances;
pub mod screenshots_grid;
pub mod search;
pub mod status;

pub fn styled_title(title: &str, highlight: bool) -> Line<'_> {
    let theme = THEME.as_ref();
    if !highlight || title.is_empty() {
        Line::from(Span::raw(title))
    } else {
        let mut chars = title.chars();
        let first = chars.next().unwrap_or_default().to_string();
        let rest: String = chars.collect();
        Line::from(vec![
            Span::styled(first, Style::default().fg(theme.accent())),
            Span::styled(rest, Style::default().fg(theme.text())),
        ])
    }
}

pub trait WidgetKey {
    fn handle_key(&mut self, key_event: &KeyEvent);
}
