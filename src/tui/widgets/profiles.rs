use crossterm::event::KeyCode;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};
use tui_widget_list::{ListBuilder, ListState as TuiListState, ListView};

use crate::instance::models::InstanceConfig;
use crate::running::{get as get_run_state, RunState};
use crate::tui::{layout::FocusedArea, theme::THEME};

use super::{search::SearchState, styled_title, WidgetKey};

fn format_last_played(last_played: Option<chrono::DateTime<chrono::Utc>>) -> String {
    let Some(dt) = last_played else {
        return "Never played".to_string();
    };
    let secs = chrono::Utc::now()
        .signed_duration_since(dt)
        .num_seconds()
        .max(0) as u64;
    match secs {
        0..=59 => "Just now".to_string(),
        60..=3599 => format!("{} minutes ago", secs / 60),
        3600..=86399 => format!("{} hours ago", secs / 3600),
        86400..=2591999 => format!("{} days ago", secs / 86400),
        2592000..=31535999 => format!("{} months ago", secs / 2592000),
        _ => "Over a year ago".to_string(),
    }
}

fn loader_color(loader: crate::instance::models::ModLoader) -> ratatui::style::Color {
    use crate::instance::models::ModLoader;
    use ratatui::style::Color;
    match loader {
        ModLoader::Vanilla => Color::Green,
        ModLoader::Fabric => Color::Rgb(0x71, 0xa5, 0xde),
        ModLoader::Forge => Color::Rgb(0xe0, 0x7b, 0x39),
        ModLoader::NeoForge => Color::Rgb(0xe0, 0x54, 0x1b),
        ModLoader::Quilt => Color::Rgb(0xa6, 0x5c, 0xcb),
    }
}

#[derive(Debug, Default)]
pub struct State {
    pub instances: Vec<InstanceConfig>,
    pub list_state: TuiListState,
    pub scrollbar_state: ScrollbarState,
    pub show_popup: bool,
    pub search: SearchState,
}

impl State {
    pub fn with_instances(instances: Vec<InstanceConfig>) -> Self {
        let count = instances.len();
        let mut s = State {
            instances,
            list_state: TuiListState::default(),
            scrollbar_state: ScrollbarState::default(),
            show_popup: false,
            search: SearchState::default(),
        };
        if count > 0 {
            s.list_state.selected = Some(0);
        }
        s.update_scrollbar();
        s
    }

    pub fn selected_instance(&self) -> Option<&InstanceConfig> {
        let filtered = self.filtered_indices();
        self.list_state
            .selected
            .and_then(|i| filtered.get(i))
            .and_then(|&idx| self.instances.get(idx))
    }

    fn filtered_indices(&self) -> Vec<usize> {
        self.instances
            .iter()
            .enumerate()
            .filter(|(_, inst)| self.search.matches(&inst.name))
            .map(|(i, _)| i)
            .collect()
    }

    fn next(&mut self) {
        let count = self.filtered_indices().len();
        if count == 0 {
            return;
        }
        self.list_state.next();
        if self.list_state.selected.unwrap_or(0) >= count {
            self.list_state.selected = Some(0);
        }
        self.update_scrollbar();
    }

    fn previous(&mut self) {
        let count = self.filtered_indices().len();
        if count == 0 {
            return;
        }
        self.list_state.previous();
        if self.list_state.selected.is_none() {
            self.list_state.selected = Some(count.saturating_sub(1));
        }
        self.update_scrollbar();
    }

    fn update_scrollbar(&mut self) {
        let filtered = self.filtered_indices();
        let count = filtered.len();
        let items = count.saturating_sub(1);
        let index = self.list_state.selected.unwrap_or(0);

        if count == 0 {
            self.list_state.selected = None;
        } else if self.list_state.selected.is_none() {
            self.list_state.selected = Some(0);
        } else if index > items {
            self.list_state.selected = Some(items);
        }

        self.scrollbar_state =
            ScrollbarState::new(items).position(self.list_state.selected.unwrap_or(0));
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
        if self.search.active {
            match key_event.code {
                KeyCode::Esc => {
                    self.search.deactivate();
                    self.list_state.selected = Some(0);
                    self.update_scrollbar();
                }
                KeyCode::Backspace => {
                    self.search.pop();
                    self.list_state.selected = Some(0);
                    self.update_scrollbar();
                }
                KeyCode::Char(c) => {
                    self.search.push(c);
                    self.list_state.selected = Some(0);
                    self.update_scrollbar();
                }
                _ => {}
            }
            return;
        }

        match key_event.code {
            KeyCode::Char('/') => {
                self.search.activate();
                self.list_state.selected = Some(0);
                self.update_scrollbar();
            }
            KeyCode::Char('a') => {
                self.show_popup = true;
                self.update_scrollbar();
            }
            KeyCode::Char('d') => {}
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

    let mut block = Block::default()
        .title(styled_title("Profiles", true))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color));

    if let Some(search_line) = state.search.title_line() {
        block = block.title_top(search_line);
    }

    let scrollbar_area = Rect {
        x: area.x + area.width.saturating_sub(1),
        y: area.y + 1,
        width: 1,
        height: area.height.saturating_sub(2),
    };

    let filtered = state.filtered_indices();
    let count = filtered.len();

    let builder = ListBuilder::new(|context| {
        let idx = filtered[context.index];
        let instance = &state.instances[idx];

        let stripe_bg = if context.index % 2 == 0 {
            Color::Reset
        } else {
            THEME.colors.row_alternate_bg
        };

        let (name_style, meta_style, bg) = if context.is_selected {
            (
                Style::default()
                    .fg(THEME.colors.row_highlight)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(THEME.colors.row_highlight),
                THEME.colors.row_background,
            )
        } else {
            (
                Style::default()
                    .fg(THEME.colors.border_focused)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(THEME.colors.border_unfocused),
                stripe_bg,
            )
        };

        let stripe = Span::styled(
            "\u{258c} ",
            Style::default().fg(loader_color(instance.loader)),
        );
        let name_line = Line::from(vec![
            stripe.clone(),
            Span::styled(instance.name.as_str(), name_style),
        ]);

        let (meta_text, meta_text_style) = match get_run_state(&instance.name) {
            Some(RunState::Running) | Some(RunState::Starting) => (
                "Playing".to_string(),
                Style::default().fg(THEME.colors.success),
            ),
            _ => (format_last_played(instance.last_played), meta_style),
        };

        let meta_line = Line::from(vec![stripe, Span::styled(meta_text, meta_text_style)]);

        let item = Text::from(vec![name_line, meta_line]).style(Style::default().bg(bg));
        (item, 2)
    });

    let list = ListView::new(builder, count).block(block);

    frame.render_stateful_widget(list, area, &mut state.list_state);

    frame.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{25b2}"))
            .style(
                Style::default()
                    .fg(THEME.colors.border_focused)
                    .add_modifier(Modifier::BOLD),
            )
            .thumb_symbol("\u{2551}")
            .track_symbol(Some(""))
            .end_symbol(Some("\u{25bc}")),
        scrollbar_area,
        &mut state.scrollbar_state,
    );
}
