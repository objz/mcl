use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{
        Block, BorderType, Borders, Cell, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
        Table, TableState,
    },
    Frame,
};

use crate::tui::layout::FocusedArea;

use super::{styled_title, WidgetKey};

#[derive(Debug, Default)]
pub struct State {
    pub instances: Vec<String>,
    pub table_state: TableState,
    pub scrollbar_state: ScrollbarState,
}

impl State {
    fn next(&mut self) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i >= self.instances.len().saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
        self.update_scrollbar();
    }

    fn previous(&mut self) {
        let i = match self.table_state.selected() {
            Some(i) => {
                if i == 0 {
                    self.instances.len().saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.table_state.select(Some(i));
        self.update_scrollbar();
    }

    fn update_scrollbar(&mut self) {
        let items = self.instances.len().saturating_sub(1);
        let index = self.table_state.selected().unwrap_or(0);

        if self.instances.is_empty() {
            self.table_state.select(None);
        } else if self.table_state.selected().is_none() {
            self.table_state.select(Some(0));
        } else if index > items {
            self.table_state.select(Some(items));
        }

        self.scrollbar_state = ScrollbarState::new(items).position(index);
    }
}

impl WidgetKey for State {
    fn handle_key(&mut self, key_event: &crossterm::event::KeyEvent) {
        match key_event.code {
            KeyCode::Char('a') => {
                self.instances.push("Test".to_string());
                self.update_scrollbar();
            }
            KeyCode::Char('d') => {
                self.instances.clear();
                self.update_scrollbar();
            }
            KeyCode::Char('j') | KeyCode::Down => self.next(),
            KeyCode::Char('k') | KeyCode::Up => self.previous(),
            _ => {}
        }
    }
}

pub fn render(frame: &mut Frame, area: Rect, focused: FocusedArea, state: &mut State) {
    let color = if focused == FocusedArea::Instances {
        Color::White
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .title(styled_title("Instances", true))
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

    let rows = state
        .instances
        .iter()
        .map(|instance| Row::new(vec![Cell::from(instance.as_str())]));

    let widths = [Constraint::Percentage(100)];

    let table = Table::new(rows, widths).row_highlight_style(
        Style::default()
            .add_modifier(Modifier::REVERSED)
            .fg(Color::Yellow),
    );

    frame.render_stateful_widget(table, table_area, &mut state.table_state);

    frame.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .thumb_symbol("┃")
            .track_symbol(Some(""))
            .end_symbol(Some("▼")),
        scrollbar_area,
        &mut state.scrollbar_state,
    );
}
