use std::path::Path;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
    },
    Frame,
};
use tui_widget_list::{ListBuilder, ListState as TuiListState, ListView};

use crate::instance::log_files::{LogFileEntry, read_log_file, scan_log_files};
use crate::tui::theme::THEME;

pub struct LogsState {
    pub entries: Vec<LogFileEntry>,
    pub list_state: TuiListState,
    pub loaded_for: Option<String>,
    pub loading: bool,
    pub viewer_focused: bool,
    pub viewer_lines: Vec<String>,
    pub viewer_scroll: usize,
    pub viewer_max_scroll: usize,
    pub scrollbar_state: ScrollbarState,
    pub viewer_scrollbar_state: ScrollbarState,
    selected_path: Option<std::path::PathBuf>,
    pending: Arc<Mutex<Option<(String, Vec<LogFileEntry>)>>>,
}

impl Default for LogsState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            list_state: TuiListState::default(),
            loaded_for: None,
            loading: false,
            viewer_focused: false,
            viewer_lines: Vec::new(),
            viewer_scroll: 0,
            viewer_max_scroll: 0,
            scrollbar_state: ScrollbarState::default(),
            viewer_scrollbar_state: ScrollbarState::default(),
            selected_path: None,
            pending: Arc::new(Mutex::new(None)),
        }
    }
}

impl LogsState {
    pub fn start_load(&mut self, instances_dir: &Path, instance_name: &str) {
        self.loading = true;
        self.loaded_for = Some(instance_name.to_string());
        self.entries.clear();
        self.list_state = TuiListState::default();
        self.viewer_lines.clear();
        self.viewer_scroll = 0;
        self.viewer_focused = false;
        self.selected_path = None;

        let dir = instances_dir.to_path_buf();
        let tag = instance_name.to_string();
        let pending = self.pending.clone();

        tokio::spawn(async move {
            let scan_dir = dir.clone();
            let scan_name = tag.clone();
            let entries =
                tokio::task::spawn_blocking(move || scan_log_files(&scan_dir, &scan_name))
                    .await
                    .unwrap_or_default();

            if let Ok(mut slot) = pending.lock() {
                *slot = Some((tag, entries));
            }
        });
    }

    pub fn drain_pending(&mut self) {
        let taken = if let Ok(mut slot) = self.pending.lock() {
            slot.take()
        } else {
            None
        };

        if let Some((instance_name, entries)) = taken {
            if self.loaded_for.as_deref() == Some(&instance_name) {
                self.entries = entries;
                self.loading = false;
                if !self.entries.is_empty() {
                    self.list_state.selected = Some(0);
                    self.load_selected();
                }
                self.update_scrollbar();
            }
        }
    }

    fn load_selected(&mut self) {
        let path = self
            .list_state
            .selected
            .and_then(|i| self.entries.get(i))
            .map(|e| e.path.clone());

        if path == self.selected_path {
            return;
        }
        self.selected_path = path.clone();
        self.viewer_scroll = 0;

        if let Some(path) = path {
            self.viewer_lines = read_log_file(&path);
        } else {
            self.viewer_lines.clear();
        }
    }

    fn update_scrollbar(&mut self) {
        let count = self.entries.len();
        let max = count.saturating_sub(1);
        let pos = self.list_state.selected.unwrap_or(0);
        self.scrollbar_state = ScrollbarState::new(max).position(pos);
    }

    fn update_viewer_scrollbar(&mut self, visible_height: usize) {
        self.viewer_max_scroll = self.viewer_lines.len().saturating_sub(visible_height);
        if self.viewer_scroll > self.viewer_max_scroll {
            self.viewer_scroll = self.viewer_max_scroll;
        }
        self.viewer_scrollbar_state = ScrollbarState::new(self.viewer_max_scroll)
            .position(self.viewer_scroll);
    }
}

pub fn handle_key(key_event: &KeyEvent, state: &mut LogsState) -> bool {
    if state.viewer_focused {
        match key_event.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if state.viewer_scroll < state.viewer_max_scroll {
                    state.viewer_scroll += 1;
                    state.viewer_scrollbar_state =
                        ScrollbarState::new(state.viewer_max_scroll).position(state.viewer_scroll);
                }
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                state.viewer_scroll = state.viewer_scroll.saturating_sub(1);
                state.viewer_scrollbar_state =
                    ScrollbarState::new(state.viewer_max_scroll).position(state.viewer_scroll);
                true
            }
            KeyCode::Char('G') => {
                state.viewer_scroll = state.viewer_max_scroll;
                state.viewer_scrollbar_state =
                    ScrollbarState::new(state.viewer_max_scroll).position(state.viewer_scroll);
                true
            }
            KeyCode::Char('g') => {
                state.viewer_scroll = 0;
                state.viewer_scrollbar_state =
                    ScrollbarState::new(state.viewer_max_scroll).position(state.viewer_scroll);
                true
            }
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => {
                state.viewer_focused = false;
                true
            }
            _ => false,
        }
    } else {
        match key_event.code {
            KeyCode::Char('j') | KeyCode::Down => {
                let count = state.entries.len();
                if count == 0 {
                    return true;
                }
                let current = state.list_state.selected.unwrap_or(0);
                state.list_state.selected = Some((current + 1).min(count - 1));
                state.load_selected();
                state.update_scrollbar();
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let current = state.list_state.selected.unwrap_or(0);
                state.list_state.selected = Some(current.saturating_sub(1));
                state.load_selected();
                state.update_scrollbar();
                true
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                if !state.viewer_lines.is_empty() {
                    state.viewer_focused = true;
                }
                true
            }
            _ => false,
        }
    }
}

pub fn render(frame: &mut Frame, area: Rect, state: &mut LogsState, is_focused: bool) {
    if state.loading {
        frame.render_widget(
            Paragraph::new("Loading logs...")
                .style(Style::default().fg(THEME.colors.text_idle)),
            area,
        );
        return;
    }

    if state.entries.is_empty() {
        let instance_name = state.loaded_for.as_deref().unwrap_or("");
        let live_lines = crate::instance_logs::get_all(instance_name);
        if !live_lines.is_empty() {
            render_live_log(frame, area, &live_lines, state);
            return;
        }
        frame.render_widget(
            Paragraph::new("No logs yet.").style(Style::default().fg(THEME.colors.text_idle)),
            area,
        );
        return;
    }

    let [list_area, viewer_area] =
        Layout::horizontal([Constraint::Length(30), Constraint::Min(0)]).areas(area);

    render_list(frame, list_area, state, is_focused);
    render_viewer(frame, viewer_area, state, is_focused);
}

fn render_list(frame: &mut Frame, area: Rect, state: &mut LogsState, is_focused: bool) {
    let list_focused = is_focused && !state.viewer_focused;
    let border_color = if list_focused {
        THEME.colors.border_focused
    } else {
        THEME.colors.border_unfocused
    };

    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let count = state.entries.len();
    if count == 0 {
        return;
    }

    let instance_name = state.loaded_for.clone().unwrap_or_default();
    let has_live = !crate::instance_logs::get_all(&instance_name).is_empty();

    let display_count = count + if has_live { 1 } else { 0 };

    let entries_snapshot: Vec<(String, bool)> = {
        let mut v = Vec::new();
        if has_live {
            v.push(("\u{25cf} Live".to_string(), true));
        }
        for e in &state.entries {
            v.push((e.name.trim_end_matches(".log").to_string(), false));
        }
        v
    };

    let builder = ListBuilder::new(move |context| {
        let (name, is_live) = &entries_snapshot[context.index];
        let show_selected = list_focused && context.is_selected;

        let style = if *is_live && show_selected {
            Style::default()
                .fg(THEME.colors.success)
                .add_modifier(Modifier::BOLD)
        } else if *is_live {
            Style::default().fg(THEME.colors.success)
        } else if show_selected {
            Style::default()
                .fg(THEME.colors.row_highlight)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(THEME.colors.text_idle)
        };

        let bg = if show_selected {
            THEME.colors.row_background
        } else {
            THEME.colors.row_alternate_bg
        };

        let item = ratatui::text::Text::from(Line::from(Span::styled(name.clone(), style)))
            .style(Style::default().bg(bg));
        (item, 1)
    });

    let list = ListView::new(builder, display_count);
    frame.render_stateful_widget(list, inner, &mut state.list_state);

    let scrollbar_area = Rect {
        x: inner.x + inner.width.saturating_sub(1),
        y: inner.y + 1,
        width: 1,
        height: inner.height.saturating_sub(2),
    };
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
}

fn render_viewer(frame: &mut Frame, area: Rect, state: &mut LogsState, is_focused: bool) {
    let viewer_focused = is_focused && state.viewer_focused;

    let instance_name = state.loaded_for.clone().unwrap_or_default();
    let is_live_selected = state.list_state.selected == Some(0)
        && !crate::instance_logs::get_all(&instance_name).is_empty();

    let lines = if is_live_selected {
        crate::instance_logs::get_all(&instance_name)
    } else {
        state.viewer_lines.clone()
    };

    if lines.is_empty() {
        frame.render_widget(
            Paragraph::new(" Select a log file")
                .style(Style::default().fg(THEME.colors.text_idle)),
            area,
        );
        return;
    }

    let visible_height = area.height as usize;
    state.update_viewer_scrollbar(visible_height);

    if is_live_selected && !state.viewer_focused {
        state.viewer_scroll = state.viewer_max_scroll;
        state.viewer_scrollbar_state =
            ScrollbarState::new(state.viewer_max_scroll).position(state.viewer_scroll);
    }

    let styled_lines: Vec<Line> = lines
        .iter()
        .skip(state.viewer_scroll)
        .take(visible_height)
        .map(|line| Line::from(Span::styled(line.as_str(), line_level_style(line))))
        .collect();

    let border_color = if viewer_focused {
        THEME.colors.border_focused
    } else {
        THEME.colors.border_unfocused
    };

    let hint = if viewer_focused {
        " Esc/h: back  j/k: scroll  g/G: top/bottom "
    } else {
        " Enter/l: view "
    };

    let block = Block::default()
        .borders(Borders::TOP)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title_top(
            Line::from(Span::styled(
                hint,
                Style::default().fg(THEME.colors.text_idle),
            ))
            .right_aligned(),
        );

    let inner = block.inner(area);
    frame.render_widget(block, area);

    frame.render_widget(Paragraph::new(styled_lines), inner);

    let scrollbar_area = Rect {
        x: inner.x + inner.width.saturating_sub(1),
        y: inner.y + 1,
        width: 1,
        height: inner.height.saturating_sub(2),
    };
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
        &mut state.viewer_scrollbar_state,
    );
}

fn render_live_log(
    frame: &mut Frame,
    area: Rect,
    lines: &[String],
    state: &mut LogsState,
) {
    let visible = area.height as usize;
    state.update_viewer_scrollbar(visible);

    let start = lines.len().saturating_sub(visible);
    let styled: Vec<Line> = lines
        .iter()
        .skip(start)
        .take(visible)
        .map(|l| Line::from(Span::styled(l.as_str(), line_level_style(l))))
        .collect();

    frame.render_widget(Paragraph::new(styled), area);
}

fn line_level_style(line: &str) -> Style {
    let upper = line.to_uppercase();
    if upper.contains("ERROR") || upper.contains("FATAL") || upper.contains("[STDERR]") {
        Style::default().fg(THEME.colors.error)
    } else if upper.contains("WARN") {
        Style::default().fg(THEME.colors.warn)
    } else if upper.contains("DEBUG") || upper.contains("TRACE") {
        Style::default().fg(THEME.colors.text_idle)
    } else {
        Style::default().fg(THEME.colors.foreground)
    }
}
