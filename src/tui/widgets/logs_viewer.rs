// split-pane log viewer: file list on the left, log content on the right.
// supports live log tailing when the instance is running, plus search
// filtering in both the file list and the viewer pane.
// log scanning runs on a background thread to avoid blocking the UI.

use std::path::Path;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};
use tui_widget_list::{ListBuilder, ListState as TuiListState, ListView};

use crate::instance::log_files::{read_log_file, scan_log_files, LogFileEntry};
use crate::config::theme::{THEME, BORDER_STYLE};

type PendingLogs = Arc<Mutex<Option<(String, Vec<LogFileEntry>)>>>;

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
    pub search: super::search::SearchState,
    pub viewer_search: super::search::SearchState,
    selected_path: Option<std::path::PathBuf>,
    pending: PendingLogs,
    rescan_counter: u8,
    instances_dir_cache: Option<std::path::PathBuf>,
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
            search: super::search::SearchState::default(),
            viewer_search: super::search::SearchState::default(),
            selected_path: None,
            pending: Arc::new(Mutex::new(None)),
            rescan_counter: 0,
            instances_dir_cache: None,
        }
    }
}

impl LogsState {
    pub fn start_load(&mut self, instances_dir: &Path, instance_name: &str) {
        self.loading = true;
        self.loaded_for = Some(instance_name.to_string());
        self.instances_dir_cache = Some(instances_dir.to_path_buf());
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
        let taken = match self.pending.lock() { Ok(mut slot) => {
            slot.take()
        } _ => {
            None
        }};

        if let Some((instance_name, entries)) = taken
            && self.loaded_for.as_deref() == Some(&instance_name) {
                let prev_selected = self.list_state.selected;
                self.entries = entries;
                self.loading = false;

                let display_count = self.display_count();

                if display_count > 0 && prev_selected.is_none() {
                    self.list_state.selected = Some(0);
                    self.load_selected_content();
                } else if let Some(sel) = prev_selected
                    && sel >= display_count && display_count > 0 {
                        self.list_state.selected = Some(display_count - 1);
                    }
                self.update_scrollbar();
            }
    }

    // periodically re-scan log files in case new ones appeared while playing.
    // only triggers every 120 ticks to avoid hammering the filesystem
    pub fn try_rescan(&mut self) {
        self.rescan_counter = self.rescan_counter.wrapping_add(1);
        if !self.rescan_counter.is_multiple_of(120) {
            return;
        }

        let (Some(dir), Some(name)) = (&self.instances_dir_cache, &self.loaded_for) else {
            return;
        };

        let dir = dir.clone();
        let tag = name.clone();
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

    // when an instance is running, a synthetic "Live" entry is injected at index 0
    fn has_live(&self) -> bool {
        let name = self.loaded_for.as_deref().unwrap_or("");
        matches!(
            crate::running::get(name),
            Some(crate::running::RunState::Running) | Some(crate::running::RunState::Starting)
        )
    }

    fn display_count(&self) -> usize {
        self.entries.len() + if self.has_live() { 1 } else { 0 }
    }

    fn is_live_selected(&self) -> bool {
        self.has_live() && self.list_state.selected == Some(0)
    }

    fn file_index_for_selected(&self) -> Option<usize> {
        let sel = self.list_state.selected?;
        let offset = if self.has_live() { 1 } else { 0 };
        if sel < offset {
            None
        } else {
            Some(sel - offset)
        }
    }

    fn load_selected_content(&mut self) {
        if self.is_live_selected() {
            self.selected_path = None;
            self.viewer_lines.clear();
            self.viewer_scroll = 0;
            return;
        }

        let path = self
            .file_index_for_selected()
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
        let count = self.display_count();
        let max = count.saturating_sub(1);
        let pos = self.list_state.selected.unwrap_or(0);
        self.scrollbar_state = ScrollbarState::new(max).position(pos);
    }

    fn update_viewer_scrollbar(&mut self, visible_height: usize, line_count: usize) {
        self.viewer_max_scroll = line_count.saturating_sub(visible_height);
        if self.viewer_scroll > self.viewer_max_scroll {
            self.viewer_scroll = self.viewer_max_scroll;
        }
        self.viewer_scrollbar_state =
            ScrollbarState::new(self.viewer_max_scroll).position(self.viewer_scroll);
    }
}

pub fn handle_key(key_event: &KeyEvent, state: &mut LogsState) -> bool {
    let shift = key_event.modifiers.contains(KeyModifiers::SHIFT);

    if state.viewer_focused {
        if state.viewer_search.active {
            match key_event.code {
                KeyCode::Esc => {
                    state.viewer_search.deactivate();
                    state.viewer_scroll = 0;
                }
                KeyCode::Backspace => {
                    state.viewer_search.pop();
                    state.viewer_scroll = 0;
                }
                KeyCode::Char(c) => {
                    state.viewer_search.push(c);
                    state.viewer_scroll = 0;
                }
                _ => {}
            }
            return true;
        }

        if key_event.code == KeyCode::Char('/') {
            state.viewer_search.activate();
            state.viewer_scroll = 0;
            return true;
        }

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
            KeyCode::Esc => {
                state.viewer_focused = false;
                true
            }
            KeyCode::Char('H') | KeyCode::Left if shift => {
                state.viewer_focused = false;
                true
            }
            _ => false,
        }
    } else {
        if state.search.active {
            match key_event.code {
                KeyCode::Esc => {
                    state.search.deactivate();
                    state.list_state.selected = Some(0);
                    state.update_scrollbar();
                }
                KeyCode::Backspace => {
                    state.search.pop();
                    state.list_state.selected = Some(0);
                    state.update_scrollbar();
                }
                KeyCode::Char(c) => {
                    state.search.push(c);
                    state.list_state.selected = Some(0);
                    state.update_scrollbar();
                }
                _ => {}
            }
            return true;
        }

        if key_event.code == KeyCode::Char('/') {
            state.search.activate();
            state.list_state.selected = Some(0);
            state.update_scrollbar();
            return true;
        }

        let display_count = state.display_count();
        match key_event.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if display_count == 0 {
                    return true;
                }
                let current = state.list_state.selected.unwrap_or(0);
                state.list_state.selected = Some((current + 1).min(display_count - 1));
                state.load_selected_content();
                state.update_scrollbar();
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let current = state.list_state.selected.unwrap_or(0);
                state.list_state.selected = Some(current.saturating_sub(1));
                state.load_selected_content();
                state.update_scrollbar();
                true
            }
            KeyCode::Enter => {
                state.viewer_focused = true;
                true
            }
            KeyCode::Char('L') | KeyCode::Right if shift => {
                state.viewer_focused = true;
                true
            }
            _ => false,
        }
    }
}

pub fn render(frame: &mut Frame, area: Rect, state: &mut LogsState, is_focused: bool) {
    let theme = THEME.as_ref();
    if state.loading {
        frame.render_widget(
            Paragraph::new("Loading logs...").style(Style::default().fg(theme.text_dim())),
            area,
        );
        return;
    }

    let has_live = state.has_live();
    let display_count = state.display_count();

    if display_count == 0 {
        frame.render_widget(
            Paragraph::new("No logs yet.").style(Style::default().fg(theme.text_dim())),
            area,
        );
        return;
    }

    if state.list_state.selected.is_none() && display_count > 0 {
        state.list_state.selected = Some(0);
        state.load_selected_content();
    }

    let [list_area, viewer_area] =
        Layout::horizontal([Constraint::Length(30), Constraint::Min(0)]).areas(area);

    render_list(frame, list_area, state, is_focused, has_live);
    render_viewer(frame, viewer_area, state, is_focused, has_live);
}

fn render_list(
    frame: &mut Frame,
    area: Rect,
    state: &mut LogsState,
    is_focused: bool,
    has_live: bool,
) {
    let theme = THEME.as_ref();
    let list_focused = is_focused && !state.viewer_focused;
    let border_color = if list_focused {
        theme.accent()
    } else {
        theme.border()
    };

    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_type(BORDER_STYLE.to_border_type())
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let display_count = state.display_count();

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
            Style::default().fg(theme.success()).add_modifier(Modifier::BOLD)
        } else if *is_live {
            Style::default().fg(theme.success())
        } else if show_selected {
            Style::default().fg(theme.accent()).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text())
        };

        let bg = if context.index % 2 == 0 {
            theme.background()
        } else {
            theme.stripe()
        };

        let selector = if show_selected {
            Span::styled("\u{258c} ", Style::default().fg(theme.accent()))
        } else {
            Span::raw("  ")
        };
        let item = ratatui::text::Text::from(Line::from(vec![selector, Span::styled(name.clone(), style)]))
            .style(Style::default().bg(bg));
        (item, 1)
    });

    let list = ListView::new(builder, display_count);
    frame.render_stateful_widget(list, inner, &mut state.list_state);

    let scrollbar_area = Rect {
        x: inner.x + inner.width.saturating_sub(0),
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
                    .fg(theme.accent())
                    .add_modifier(Modifier::BOLD),
            )
            .thumb_symbol("\u{2551}")
            .track_symbol(Some(""))
            .end_symbol(Some("\u{25bc}")),
        scrollbar_area,
        &mut state.scrollbar_state,
    );
}

fn render_viewer(
    frame: &mut Frame,
    area: Rect,
    state: &mut LogsState,
    _is_focused: bool,
    has_live: bool,
) {
    let theme = THEME.as_ref();
    let is_live = has_live && state.list_state.selected == Some(0);

    let all_lines: Vec<String> = if is_live {
        let name = state.loaded_for.as_deref().unwrap_or("");
        crate::instance_logs::get_all(name)
    } else {
        state.viewer_lines.clone()
    };

    let lines: Vec<&String> = all_lines
        .iter()
        .filter(|l| state.viewer_search.matches(l))
        .collect();

    let visible_height = area.height as usize;
    // auto-scroll: if the user was already at the bottom, keep following
    // new lines as they come in (like `tail -f` behavior)
    let was_at_bottom = state.viewer_scroll >= state.viewer_max_scroll.saturating_sub(1);
    state.update_viewer_scrollbar(visible_height, lines.len());

    if is_live && was_at_bottom && !state.viewer_search.active {
        state.viewer_scroll = state.viewer_max_scroll;
        state.viewer_scrollbar_state =
            ScrollbarState::new(state.viewer_max_scroll).position(state.viewer_scroll);
    }

    if lines.is_empty() {
        return;
    }

    let search = &state.viewer_search;
    let styled_lines: Vec<Line> = lines
        .iter()
        .skip(state.viewer_scroll)
        .take(visible_height)
        .map(|line| search.highlight_line(line, line_level_style(line)))
        .collect();

    frame.render_widget(Paragraph::new(styled_lines), area);

    let scrollbar_area = Rect {
        x: area.x + area.width.saturating_sub(0),
        y: area.y + 1,
        width: 1,
        height: area.height.saturating_sub(2),
    };
    frame.render_stateful_widget(
        Scrollbar::default()
            .orientation(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("\u{25b2}"))
            .style(
                Style::default()
                    .fg(theme.accent())
                    .add_modifier(Modifier::BOLD),
            )
            .thumb_symbol("\u{2551}")
            .track_symbol(Some(""))
            .end_symbol(Some("\u{25bc}")),
        scrollbar_area,
        &mut state.viewer_scrollbar_state,
    );
}

// color-code log lines by severity so errors actually stand out
// instead of drowning in a wall of white text
fn line_level_style(line: &str) -> Style {
    let theme = THEME.as_ref();
    let upper = line.to_uppercase();
    if upper.contains("ERROR") || upper.contains("FATAL") || upper.contains("[STDERR]") {
        Style::default().fg(theme.error())
    } else if upper.contains("WARN") {
        Style::default().fg(theme.warning())
    } else if upper.contains("DEBUG") || upper.contains("TRACE") {
        Style::default().fg(theme.text_dim())
    } else {
        Style::default().fg(theme.text())
    }
}
