use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Widget,
    },
    Frame,
};

use crate::instance::models::InstanceConfig;
use crate::tui::{layout::FocusedArea, theme::THEME};

use super::{popups, styled_title, WidgetKey};

#[derive(Debug, Default)]
pub struct State {
    pub instances: Vec<InstanceConfig>,
    pub list_state: ListState,
    pub scrollbar_state: ScrollbarState,
    pub show_popup: bool,
    pub search_mode: bool,
    pub search_query: String,
}

impl State {
    pub fn with_instances(instances: Vec<InstanceConfig>) -> Self {
        let count = instances.len();
        let mut s = State {
            instances,
            list_state: ListState::default(),
            scrollbar_state: ScrollbarState::default(),
            show_popup: false,
            search_mode: false,
            search_query: String::new(),
        };
        if count > 0 {
            s.list_state.select(Some(0));
        }
        s.update_scrollbar();
        s
    }

    pub fn selected_instance(&self) -> Option<&InstanceConfig> {
        let filtered = self.filtered_indices();
        self.list_state
            .selected()
            .and_then(|i| filtered.get(i))
            .and_then(|&idx| self.instances.get(idx))
    }

    fn filtered_indices(&self) -> Vec<usize> {
        if self.search_query.is_empty() {
            (0..self.instances.len()).collect()
        } else {
            let q = self.search_query.to_lowercase();
            self.instances
                .iter()
                .enumerate()
                .filter(|(_, inst)| inst.name.to_lowercase().contains(&q))
                .map(|(i, _)| i)
                .collect()
        }
    }

    fn next(&mut self) {
        let count = self.filtered_indices().len();
        if count == 0 {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i >= count.saturating_sub(1) {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
        self.update_scrollbar();
    }

    fn previous(&mut self) {
        let count = self.filtered_indices().len();
        if count == 0 {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => {
                if i == 0 {
                    count.saturating_sub(1)
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.list_state.select(Some(i));
        self.update_scrollbar();
    }

    fn update_scrollbar(&mut self) {
        let filtered = self.filtered_indices();
        let count = filtered.len();
        let items = count.saturating_sub(1);
        let index = self.list_state.selected().unwrap_or(0);

        if count == 0 {
            self.list_state.select(None);
        } else if self.list_state.selected().is_none() {
            self.list_state.select(Some(0));
        } else if index > items {
            self.list_state.select(Some(items));
        }

        self.scrollbar_state = ScrollbarState::new(items).position(index);
    }

    pub fn wants_popup(&self) -> bool {
        self.show_popup
    }

    pub fn remove_instance(&mut self, name: &str) {
        let before = self.instances.len();
        self.instances.retain(|i| i.name != name);
        let after = self.instances.len();
        if after < before {
            self.update_scrollbar();
        }
    }

    pub fn add_instance(&mut self, instance: InstanceConfig) {
        self.instances.push(instance);
        self.update_scrollbar();
    }
}

impl WidgetKey for State {
    fn handle_key(&mut self, key_event: &crossterm::event::KeyEvent) {
        match key_event.code {
            KeyCode::Char('/') if !self.search_mode => {
                self.search_mode = true;
                self.list_state.select(Some(0));
                self.update_scrollbar();
            }
            KeyCode::Esc if self.search_mode => {
                self.search_mode = false;
                self.search_query.clear();
                self.list_state.select(Some(0));
                self.update_scrollbar();
            }
            KeyCode::Backspace if self.search_mode => {
                self.search_query.pop();
                self.list_state.select(Some(0));
                self.update_scrollbar();
            }
            KeyCode::Char(c) if self.search_mode && c != 'j' && c != 'k' => {
                self.search_query.push(c);
                self.list_state.select(Some(0));
                self.update_scrollbar();
            }
            KeyCode::Char('a') => {
                self.show_popup = true;
                self.update_scrollbar();
            }
            KeyCode::Char('d') => {
                // Deletion handled by layout.rs which calls remove_instance
            }
            KeyCode::Char('j') | KeyCode::Down => self.next(),
            KeyCode::Char('k') | KeyCode::Up => self.previous(),
            _ => {}
        }
    }
}

pub fn render(frame: &mut Frame, area: Rect, focused: FocusedArea, state: &mut State) {
    let color = if focused == FocusedArea::Profiles {
        THEME.colors.border_focused
    } else {
        THEME.colors.border_unfocused
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

    let inner_area = block.inner(area);

    let (list_area, search_area) = if state.search_mode {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner_area);
        (chunks[0], Some(chunks[1]))
    } else {
        (inner_area, None)
    };

    let filtered = state.filtered_indices();
    let items: Vec<ListItem> = filtered
        .iter()
        .map(|&idx| {
            let instance = &state.instances[idx];

            let name_line = Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    instance.name.as_str(),
                    Style::default()
                        .fg(THEME.colors.border_focused)
                        .add_modifier(Modifier::BOLD),
                ),
            ]);
            let info_line = Line::from(vec![
                Span::raw("   "),
                Span::styled(
                    format!("{} \u{00b7} {}", instance.game_version, instance.loader),
                    Style::default().fg(THEME.colors.border_unfocused),
                ),
            ]);
            let spacer = Line::from("");

            ListItem::new(Text::from(vec![name_line, info_line, spacer]))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(Style::default().bg(THEME.colors.row_alternate_bg))
        .highlight_symbol("");

    frame.render_stateful_widget(list, list_area, &mut state.list_state);

    if let Some(sa) = search_area {
        let search_text = format!("/ {}\u{2588}", state.search_query);
        let paragraph =
            Paragraph::new(search_text).style(Style::default().fg(THEME.colors.border_focused));
        paragraph.render(sa, frame.buffer_mut());
    }

    frame.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{25b2}"))
            .style(
                Style::default()
                    .fg(THEME.colors.border_focused)
                    .add_modifier(Modifier::BOLD),
            )
            .thumb_symbol("\u{2503}")
            .track_symbol(Some(""))
            .end_symbol(Some("\u{25bc}")),
        scrollbar_area,
        &mut state.scrollbar_state,
    );

    if state.show_popup {
        let popup_area = popups::new_instance::popup_rect(frame.area());
        popups::new_instance::render(frame, popup_area, focused);
    }
}
