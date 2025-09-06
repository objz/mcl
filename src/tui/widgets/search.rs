use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::tui::theme::THEME;

#[derive(Debug, Default, Clone)]
pub struct SearchState {
    pub query: String,
    pub active: bool,
}

impl SearchState {
    pub fn activate(&mut self) {
        self.active = true;
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.query.clear();
    }

    pub fn push(&mut self, c: char) {
        self.query.push(c);
    }

    pub fn pop(&mut self) {
        self.query.pop();
    }

    pub fn is_empty(&self) -> bool {
        self.query.is_empty()
    }

    pub fn matches(&self, text: &str) -> bool {
        if self.query.is_empty() {
            return true;
        }
        text.to_lowercase().contains(&self.query.to_lowercase())
    }

    pub fn title_line(&self) -> Option<Line<'static>> {
        if !self.active && self.query.is_empty() {
            return None;
        }

        let dim = Style::default().fg(THEME.colors.border_unfocused);
        let accent = Style::default()
            .fg(THEME.colors.border_focused)
            .add_modifier(Modifier::BOLD);

        let mut spans = vec![
            Span::styled(" / ", dim),
            Span::styled(self.query.clone(), accent),
        ];

        if self.active {
            spans.push(Span::styled("\u{2588}", accent));
        }

        spans.push(Span::raw(" "));

        Some(Line::from(spans).right_aligned())
    }
}
