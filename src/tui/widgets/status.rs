use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, Gauge, Paragraph},
    Frame,
};
use throbber_widgets_tui::{Throbber, ThrobberState};

use crate::tui::layout::FocusedArea;
use crate::tui::progress::PROGRESS;
use crate::tui::theme::THEME;

use super::styled_title;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    focused: FocusedArea,
    throbber_state: &mut ThrobberState,
) {
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

    let state = match PROGRESS.lock() {
        Ok(s) => s.clone(),
        Err(_) => {
            let idle = Paragraph::new(Span::styled("Ready", Style::default().fg(Color::DarkGray)));
            frame.render_widget(idle.block(block), area);
            return;
        }
    };

    if state.current_action.is_none() {
        let idle = Paragraph::new(Span::styled("Ready", Style::default().fg(Color::DarkGray)));
        frame.render_widget(idle.block(block), area);
        return;
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let action_text = state.current_action.as_deref().unwrap_or("");
    let sub_text = state.sub_action.as_deref().unwrap_or("");

    match state.progress {
        Some((current, total)) if total > 0 => {
            let chunks = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(inner);

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
            frame.render_widget(gauge, chunks[0]);

            frame.render_widget(
                Paragraph::new(action_text).style(Style::default().fg(THEME.colors.foreground)),
                chunks[1],
            );

            if !sub_text.is_empty() {
                frame.render_widget(
                    Paragraph::new(sub_text)
                        .style(Style::default().fg(THEME.colors.border_unfocused)),
                    chunks[2],
                );
            }
        }
        _ => {
            let chunks =
                Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);

            let throbber = Throbber::default()
                .label(action_text)
                .style(Style::default().fg(THEME.colors.foreground))
                .throbber_style(
                    Style::default()
                        .fg(THEME.colors.border_focused)
                        .add_modifier(Modifier::BOLD),
                );
            frame.render_stateful_widget(throbber, chunks[0], throbber_state);

            if !sub_text.is_empty() {
                frame.render_widget(
                    Paragraph::new(sub_text)
                        .style(Style::default().fg(THEME.colors.border_unfocused)),
                    chunks[1],
                );
            }
        }
    }
}
