// shared utilities for popup widgets: layout helpers, word wrapping, keybind rendering.
// individual popup types live in their own submodules.

pub mod base;
pub mod confirm;
pub mod error;
pub mod import_modpack;
mod load_state;
pub mod new_instance;

pub use load_state::LoadState;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};

pub(crate) fn compare_game_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parse_parts = |version: &str| {
        version
            .split('.')
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    let a_parts = parse_parts(a);
    let b_parts = parse_parts(b);
    for (a, b) in a_parts.iter().zip(&b_parts) {
        match a.cmp(b) {
            std::cmp::Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    a_parts.len().cmp(&b_parts.len())
}

// figures out the (width, height) a text block will need after word wrapping.
// used to size popups before rendering so they fit their content snugly.
pub fn word_wrap_size(text: &str, max_inner_width: usize) -> (usize, usize) {
    if text.is_empty() || max_inner_width == 0 {
        return (0, 1);
    }

    let mut widest_line: usize = 0;
    let mut lines = 0;
    for logical_line in text.split('\n') {
        let mut current_line_len = 0;
        lines += 1;
        for word in logical_line.split_whitespace() {
            let word_len = Span::raw(word).width().min(max_inner_width);
            if current_line_len == 0 {
                current_line_len = word_len;
            } else if current_line_len + 1 + word_len <= max_inner_width {
                current_line_len += 1 + word_len;
            } else {
                widest_line = widest_line.max(current_line_len);
                lines += 1;
                current_line_len = word_len;
            }
        }
        widest_line = widest_line.max(current_line_len);
    }

    (widest_line, lines)
}

pub fn top_right_rect(frame: Rect, inner_w: usize, inner_h: usize) -> Rect {
    let popup_w = (inner_w + 2) as u16;
    let popup_h = (inner_h + 2) as u16;
    let popup_w = popup_w.min(frame.width.saturating_sub(4));
    let popup_h = popup_h.min(frame.height.saturating_sub(2));
    let x = frame.width.saturating_sub(popup_w + 2);
    let y = 1u16;
    Rect {
        x,
        y,
        width: popup_w,
        height: popup_h,
    }
}

pub fn keybind_line(binds: &[(&str, &str)]) -> ratatui::text::Line<'static> {
    use crate::config::theme::THEME;
    use ratatui::{
        style::{Modifier, Style},
        text::{Line, Span},
    };
    let theme = THEME.as_ref();
    let key_style = Style::default()
        .fg(theme.accent())
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(theme.text());

    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, (key, label)) in binds.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", label_style));
        }
        spans.push(Span::styled(format!("[{}]", key), key_style));
        if !label.is_empty() {
            spans.push(Span::styled(label.to_string(), label_style));
        }
    }
    Line::from(spans)
}

// same as keybind_line but wraps to multiple rows when the panel is too narrow
// to fit everything on one line
pub fn keybind_lines_wrapped(
    binds: &[(&str, &str)],
    max_width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    use crate::config::theme::THEME;
    use ratatui::{
        style::{Modifier, Style},
        text::{Line, Span},
    };
    let theme = THEME.as_ref();
    let key_style = Style::default()
        .fg(theme.accent())
        .add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(theme.text());

    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width: usize = 0;

    for (i, (key, label)) in binds.iter().enumerate() {
        let sep_w = if i > 0 && !current_spans.is_empty() {
            2
        } else {
            0
        };
        let item_w = Span::raw(format!("[{key}]{label}")).width();
        let needed = sep_w + item_w;

        if !current_spans.is_empty() && current_width + needed > max_width as usize {
            rows.push(Line::from(current_spans).right_aligned());
            current_spans = Vec::new();
            current_width = 0;
        }

        if !current_spans.is_empty() {
            current_spans.push(Span::styled("  ", label_style));
            current_width += 2;
        }

        current_spans.push(Span::styled(format!("[{}]", key), key_style));
        if !label.is_empty() {
            current_spans.push(Span::styled(label.to_string(), label_style));
        }
        current_width += item_w;
    }

    if !current_spans.is_empty() {
        rows.push(Line::from(current_spans).right_aligned());
    }

    rows
}

pub fn render_keybind_overflow(frame: &mut Frame, area: Rect, lines: &[Line<'static>]) -> Rect {
    let overflow = lines.len().saturating_sub(1);
    if overflow == 0 {
        return area;
    }

    let [content, footer] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(overflow as u16)]).areas(area);
    frame.render_widget(
        Paragraph::new(lines[..overflow].to_vec()).alignment(Alignment::Right),
        footer,
    );
    content
}

#[cfg(test)]
#[path = "../../tests/widgets/popups/mod.rs"]
mod tests;
