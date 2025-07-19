use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Gauge, Paragraph},
    Frame,
};

use crate::tui::layout::FocusedArea;
use crate::tui::progress::PROGRESS;

use super::styled_title;

pub fn render(frame: &mut Frame, area: Rect, focused: FocusedArea) {
    let border_color = if focused == FocusedArea::Status {
        Color::White
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .title(styled_title("Status", true))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let state = match PROGRESS.lock() {
        Ok(s) => s.clone(),
        Err(_) => {
            let idle = Paragraph::new(Span::styled("Ready", Style::default().fg(Color::DarkGray)));
            frame.render_widget(idle, inner);
            return;
        }
    };

    if state.current_action.is_none() {
        let idle = Paragraph::new(Span::styled("Ready", Style::default().fg(Color::DarkGray)));
        frame.render_widget(idle, inner);
        return;
    }

    let action_text = state.current_action.as_deref().unwrap_or("");
    let sub_text = state.sub_action.as_deref().unwrap_or("");

    let has_progress = state.progress.is_some();
    let constraints = if has_progress {
        vec![
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    } else {
        vec![Constraint::Length(1), Constraint::Length(1)]
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    let mut chunk_idx = 0;

    if has_progress {
        if let Some((current, total)) = state.progress {
            let ratio = if total > 0 {
                (current as f64 / total as f64).min(1.0)
            } else {
                0.0
            };
            let pct = (ratio * 100.0) as u16;
            let gauge = Gauge::default()
                .gauge_style(
                    Style::default()
                        .fg(Color::Green)
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                )
                .percent(pct);
            frame.render_widget(gauge, chunks[chunk_idx]);
            chunk_idx += 1;
        }
    }

    let action_line = Paragraph::new(Line::from(vec![Span::styled(
        action_text,
        Style::default().fg(Color::White),
    )]));
    frame.render_widget(action_line, chunks[chunk_idx]);
    chunk_idx += 1;

    if !sub_text.is_empty() && chunk_idx < chunks.len() {
        let sub_line = Paragraph::new(Line::from(vec![Span::styled(
            sub_text,
            Style::default().fg(Color::DarkGray),
        )]));
        frame.render_widget(sub_line, chunks[chunk_idx]);
    }
}
