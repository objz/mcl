// reusable incremental search state used across multiple widgets.
// handles case-insensitive filtering and inline match highlighting.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::config::theme::THEME;

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

    // splits a line into spans, bolding+underlining the parts that match
    // the query so they pop out visually
    pub fn highlight_line<'a>(&self, text: &'a str, base_style: Style) -> Line<'a> {
        if self.query.is_empty() {
            return Line::from(Span::styled(text, base_style));
        }

        let query_lower = self.query.to_lowercase();
        let text_lower = text.to_lowercase();
        let mut spans = Vec::new();
        let mut last = 0;

        for (start, _) in text_lower.match_indices(&query_lower) {
            if start > last {
                spans.push(Span::styled(&text[last..start], base_style));
            }
            spans.push(Span::styled(
                &text[start..start + self.query.len()],
                base_style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ));
            last = start + self.query.len();
        }

        if last < text.len() {
            spans.push(Span::styled(&text[last..], base_style));
        }

        if spans.is_empty() {
            Line::from(Span::styled(text, base_style))
        } else {
            Line::from(spans)
        }
    }

    // renders the "/ query█" indicator in the block title bar
    pub fn title_line(&self) -> Option<Line<'static>> {
        if !self.active && self.query.is_empty() {
            return None;
        }

        let theme = THEME.as_ref();
        let dim = Style::default().fg(theme.text_dim());
        let accent = Style::default()
            .fg(theme.text_dim())
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
