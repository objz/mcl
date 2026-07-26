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

    // exit search mode but keep the filter active so the user can
    // navigate the filtered results
    pub fn confirm(&mut self) {
        self.active = false;
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

    // splits text into spans, bolding+underlining the parts that match
    // the query so every searchable widget can use the same styling
    pub fn highlight_spans(&self, text: &str, base_style: Style) -> Vec<Span<'static>> {
        if self.query.is_empty() {
            return vec![Span::styled(text.to_owned(), base_style)];
        }

        let query_lower = self.query.to_lowercase();
        let text_lower = text.to_lowercase();
        let mut spans = Vec::new();
        let mut last = 0;

        for (start, _) in text_lower.match_indices(&query_lower) {
            let end = start + query_lower.len();
            if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
                continue;
            }
            if start > last {
                spans.push(Span::styled(text[last..start].to_owned(), base_style));
            }
            spans.push(Span::styled(
                text[start..end].to_owned(),
                base_style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ));
            last = end;
        }

        if last < text.len() {
            spans.push(Span::styled(text[last..].to_owned(), base_style));
        }

        if spans.is_empty() {
            spans.push(Span::styled(text.to_owned(), base_style));
        }
        spans
    }

    pub fn highlight_line(&self, text: &str, base_style: Style) -> Line<'static> {
        Line::from(self.highlight_spans(text, base_style))
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

#[cfg(test)]
mod tests;
