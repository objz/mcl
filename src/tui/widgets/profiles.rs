use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    text::Text,
    widgets::{
        Block, BorderType, Borders, Cell, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Table, TableState,
    },
    Frame,
};

use crate::{config::SETTINGS, tui::layout::FocusedArea};

use super::{popups, styled_title, WidgetKey};

#[derive(Debug, Default)]
pub struct State {
    pub profiles: Vec<Data>,
    pub table_state: TableState,
    pub scrollbar_state: ScrollbarState,
    pub show_popup: bool,
}

#[derive(Debug, Default)]
pub struct Data {
    pub title: String,
    pub id: String,
    pub running: bool,
}

impl State {
    fn next(&mut self) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.profiles.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            _none => 0,
        };
        self.table_state.select(Some(i));
        self.update_scrollbar();
    }

    fn previous(&mut self) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.profiles.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            _none => 0,
        };
        self.table_state.select(Some(i));
        self.update_scrollbar();
    }

    fn update_scrollbar(&mut self) {
        let items = self.profiles.len().saturating_sub(1);
        let index = self.table_state.selected().unwrap_or(0);

        if self.profiles.is_empty() {
            self.table_state.select(None);
        } else if self.table_state.selected().is_none() {
            self.table_state.select(Some(0));
        } else if index > items {
            self.table_state.select(Some(items));
        }

        self.scrollbar_state = ScrollbarState::new(items).position(index);
    }

    pub fn wants_popup(&self) -> bool {
        self.show_popup
    }
}

impl WidgetKey for State {
    fn handle_key(&mut self, key_event: &crossterm::event::KeyEvent) {
        match key_event.code {
            KeyCode::Char('a') => {
                self.show_popup = true;
                self.update_scrollbar();
            }
            KeyCode::Char('d') => {
                self.profiles.clear();
                self.update_scrollbar();
            }
            KeyCode::Char('j') | KeyCode::Down => self.next(),
            KeyCode::Char('k') | KeyCode::Up => self.previous(),
            _ => {}
        }
    }
}

pub fn render(frame: &mut Frame, area: Rect, focused: FocusedArea, state: &mut State) {
    let color = if focused == FocusedArea::Profiles {
        SETTINGS.colors.border_focused
    } else {
        SETTINGS.colors.border_unfocused
    };

    let block = Block::default()
        .title(styled_title("Profiles", true))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color));

    frame.render_widget(&block, area);

    let scrollbar_area = Rect {
        x: area.x + area.width.saturating_sub(1),
        y: area.y + 1,
        width: 1,
        height: area.height.saturating_sub(2),
    };

    let table_area = block.inner(area);

    let rows = state.profiles.iter().enumerate().map(|(i, data)| {
        let status = if data.running { "Running" } else { "Stopped" };

        let background_color = if i % 2 == 0 {
            SETTINGS.colors.row_background
        } else {
            SETTINGS.colors.row_alternate_bg
        };

        Row::new(vec![
            Cell::from(Text::from(format!("\n{}\n", data.title))),
            Cell::from(Text::from(format!("\n{}\n", data.id))),
            Cell::from(Text::from(format!("\n{}\n", status))),
        ])
        .height(4)
        .style(Style::default().bg(background_color))
    });

    let widths = [
        Constraint::Percentage(40),
        Constraint::Percentage(30),
        Constraint::Percentage(30),
    ];

    let table = Table::new(rows, widths)
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(SETTINGS.colors.row_highlight),
        )
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

    frame.render_stateful_widget(table, table_area, &mut state.table_state);

    frame.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .style(
                Style::default()
                    .fg(SETTINGS.colors.border_focused)
                    .add_modifier(Modifier::BOLD),
            )
            .thumb_symbol("┃")
            .track_symbol(Some(""))
            .end_symbol(Some("▼")),
        scrollbar_area,
        &mut state.scrollbar_state,
    );

    if state.show_popup {
        let popup_area = Rect {
            x: frame.area().width / 4,
            y: frame.area().height / 3,
            width: frame.area().width / 2,
            height: frame.area().height / 3,
        };
        popups::new_instance::render(frame, popup_area, focused);
    }
}
