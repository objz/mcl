use crossterm::event::KeyEvent;
use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

pub mod account;
pub mod content;
pub mod details;
pub mod instances;
pub mod status;

pub fn styled_title(title: &str, highlight: bool) -> Line {
    if !highlight || title.is_empty() {
        Line::from(Span::raw(title))
    } else {
        let mut chars = title.chars();
        let first = chars.next().unwrap_or_default().to_string();
        let rest: String = chars.collect();
        Line::from(vec![
            Span::styled(first, Style::default().fg(Color::Yellow)),
            Span::raw(rest),
        ])
    }
}

pub trait WidgetKey {
    fn handle_key(&mut self, key_event: &KeyEvent); 
}
