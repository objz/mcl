pub mod base;
pub mod confirm;
pub mod error;
pub mod import_modpack;
pub mod new_instance;

use ratatui::layout::Rect;

/// Calculates the minimum inner dimensions needed to display `text` with word-boundary wrapping
/// inside a box of `max_inner_width` columns.
///
/// Returns `(actual_inner_width, line_count)` where:
/// - `actual_inner_width` is the width of the widest wrapped line (never exceeds `max_inner_width`)
/// - `line_count` is how many lines are needed
///
/// Add 2 to each dimension for borders: popup_w = actual_inner_width + 2, popup_h = line_count + 2.
pub fn word_wrap_size(text: &str, max_inner_width: usize) -> (usize, usize) {
    if text.is_empty() || max_inner_width == 0 {
        return (0, 1);
    }

    let mut lines: usize = 1;
    let mut current_line_len: usize = 0;
    let mut widest_line: usize = 0;

    for word in text.split_whitespace() {
        let word_len = word.len().min(max_inner_width);
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
    use crate::tui::theme::THEME;
    use ratatui::{
        style::{Modifier, Style},
        text::{Line, Span},
    };

    let mut key_style = Style::default().fg(THEME.popup.keybind_active_fg);
    if THEME.popup.keybind_active_bold {
        key_style = key_style.add_modifier(Modifier::BOLD);
    }
    let dim_style = Style::default().fg(THEME.popup.keybind_inactive_fg);

    let mut spans: Vec<Span<'static>> = Vec::new();
    for (i, (key, label)) in binds.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", dim_style));
        }
        spans.push(Span::styled(format!("[{}]", key), key_style));
        if !label.is_empty() {
            spans.push(Span::styled(label.to_string(), dim_style));
        }
    }
    Line::from(spans)
}

pub fn keybind_lines_wrapped(
    binds: &[(&str, &str)],
    max_width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    use crate::tui::theme::THEME;
    use ratatui::{
        style::{Modifier, Style},
        text::{Line, Span},
    };

    let mut key_style = Style::default().fg(THEME.popup.keybind_active_fg);
    if THEME.popup.keybind_active_bold {
        key_style = key_style.add_modifier(Modifier::BOLD);
    }
    let dim_style = Style::default().fg(THEME.popup.keybind_inactive_fg);

    let mut rows: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width: usize = 0;

    for (i, (key, label)) in binds.iter().enumerate() {
        let sep_w = if i > 0 && !current_spans.is_empty() {
            2
        } else {
            0
        };
        let item_w = key.len() + 2 + label.len();
        let needed = sep_w + item_w;

        if !current_spans.is_empty() && current_width + needed > max_width as usize {
            rows.push(Line::from(current_spans).right_aligned());
            current_spans = Vec::new();
            current_width = 0;
        }

        if !current_spans.is_empty() {
            current_spans.push(Span::styled("  ", dim_style));
            current_width += 2;
        }

        current_spans.push(Span::styled(format!("[{}]", key), key_style));
        if !label.is_empty() {
            current_spans.push(Span::styled(label.to_string(), dim_style));
        }
        current_width += item_w;
    }

    if !current_spans.is_empty() {
        rows.push(Line::from(current_spans).right_aligned());
    }

    rows
}
