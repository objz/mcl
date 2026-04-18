use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};
use tui_widget_list::{ListBuilder, ListState as TuiListState, ListView};

use crate::instance::content::mods::{ContentEntry, IconCell};
use crate::config::theme::THEME;

type PendingContent = Arc<Mutex<Option<(String, Vec<ContentEntry>)>>>;
type SnapshotRow = (String, String, bool, Option<Vec<Vec<IconCell>>>);

struct CachedList {
    entries: Vec<ContentEntry>,
    selected: Option<usize>,
}

pub struct ContentListState {
    pub entries: Vec<ContentEntry>,
    pub list_state: TuiListState,
    pub scrollbar_state: ScrollbarState,
    pub loaded_for: Option<String>,
    pub loading: bool,
    pub search: crate::tui::widgets::search::SearchState,
    cache: HashMap<String, CachedList>,
    pending: PendingContent,
    check_counter: u16,
    watched_dir: Option<std::path::PathBuf>,
    last_dir_mtime: Option<std::time::SystemTime>,
}

impl Default for ContentListState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            list_state: TuiListState::default(),
            scrollbar_state: ScrollbarState::default(),
            loaded_for: None,
            loading: false,
            search: crate::tui::widgets::search::SearchState::default(),
            cache: HashMap::new(),
            pending: Arc::new(Mutex::new(None)),
            check_counter: 0,
            watched_dir: None,
            last_dir_mtime: None,
        }
    }
}

impl ContentListState {
    /// Check if the content directory changed (files added/removed).
    /// Only triggers a rescan when the directory's modification time differs.
    pub fn try_rescan(&mut self) {
        self.check_counter = self.check_counter.wrapping_add(1);
        // Check every ~2 seconds (120 ticks at 16ms)
        if !self.check_counter.is_multiple_of(120) {
            return;
        }

        let Some(dir) = &self.watched_dir else {
            return;
        };

        let current_mtime = std::fs::metadata(dir)
            .ok()
            .and_then(|m| m.modified().ok());

        if current_mtime != self.last_dir_mtime {
            self.last_dir_mtime = current_mtime;
            // Directory changed — invalidate so next render triggers start_load
            if let Some(name) = &self.loaded_for {
                self.cache.remove(name);
                self.loaded_for = None;
            }
        }
    }

    /// Set the directory to watch for changes. Call after start_load.
    pub fn watch_dir(&mut self, dir: std::path::PathBuf) {
        self.last_dir_mtime = std::fs::metadata(&dir).ok().and_then(|m| m.modified().ok());
        self.watched_dir = Some(dir);
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| self.search.matches(&e.name))
            .map(|(i, _)| i)
            .collect()
    }
}

impl ContentListState {
    pub fn start_load<F>(&mut self, instances_dir: &Path, instance_name: &str, scan_fn: F)
    where
        F: FnOnce(&Path, &str) -> Vec<ContentEntry> + Send + 'static,
    {
        if let Some(prev) = self.loaded_for.take() {
            if !self.entries.is_empty() {
                self.cache.insert(
                    prev,
                    CachedList {
                        entries: std::mem::take(&mut self.entries),
                        selected: self.list_state.selected,
                    },
                );
            }
        }

        if let Some(cached) = self.cache.remove(instance_name) {
            self.entries = cached.entries;
            self.list_state.selected = cached.selected;
            self.loading = false;
        } else {
            self.entries.clear();
            self.list_state = TuiListState::default();
            self.loading = true;
        }

        self.loaded_for = Some(instance_name.to_string());
        self.update_scrollbar();

        let dir = instances_dir.to_path_buf();
        let tag = instance_name.to_string();
        let pending = self.pending.clone();

        tokio::spawn(async move {
            let scan_dir = dir.clone();
            let scan_name = tag.clone();
            let entries = tokio::task::spawn_blocking(move || scan_fn(&scan_dir, &scan_name))
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
            if self.loaded_for.as_deref() == Some(&instance_name) || instance_name.is_empty() {
                let selected_name = self
                    .list_state
                    .selected
                    .and_then(|i| self.entries.get(i))
                    .map(|e| e.name.clone());

                self.entries = entries;
                self.loading = false;

                if !self.entries.is_empty() {
                    let new_index = selected_name
                        .and_then(|name| self.entries.iter().position(|e| e.name == name))
                        .or(self.list_state.selected)
                        .map(|i| i.min(self.entries.len().saturating_sub(1)))
                        .unwrap_or(0);
                    self.list_state.selected = Some(new_index);
                } else {
                    self.list_state.selected = None;
                }
                self.update_scrollbar();
            }
        }
    }

    fn update_scrollbar(&mut self) {
        let count = self.entries.len();
        let max = count.saturating_sub(1);
        let pos = self.list_state.selected.unwrap_or(0);
        self.scrollbar_state = ScrollbarState::new(max).position(pos);
    }

    pub fn toggle_selected(&mut self) {
        let Some(index) = self.list_state.selected else {
            return;
        };
        let Some(entry) = self.entries.get(index) else {
            return;
        };

        let new_path = if entry.enabled {
            let fname = match entry.path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => return,
            };
            let mut p = entry.path.clone();
            p.set_file_name(format!("{fname}.disabled"));
            p
        } else {
            let fname = match entry.path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => return,
            };
            let mut p = entry.path.clone();
            p.set_file_name(fname.trim_end_matches(".disabled"));
            p
        };

        match std::fs::rename(&entry.path, &new_path) {
            Ok(()) => {
                let entry = &mut self.entries[index];
                entry.enabled = !entry.enabled;
                entry.path = new_path;
            }
            Err(e) => {
                tracing::error!("Failed to toggle '{}': {}", entry.file_stem, e);
            }
        }
    }
}

fn handle_search_keys(key_event: &KeyEvent, state: &mut ContentListState) -> bool {
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
    false
}

pub fn handle_key_no_toggle(key_event: &KeyEvent, state: &mut ContentListState) -> bool {
    if handle_search_keys(key_event, state) {
        return true;
    }
    let filtered = state.filtered_indices();
    let count = filtered.len();
    match key_event.code {
        KeyCode::Char('j') | KeyCode::Down => {
            if count == 0 {
                return true;
            }
            let current = state.list_state.selected.unwrap_or(0);
            state.list_state.selected = Some((current + 1).min(count - 1));
            state.update_scrollbar();
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let current = state.list_state.selected.unwrap_or(0);
            state.list_state.selected = Some(current.saturating_sub(1));
            state.update_scrollbar();
            true
        }
        KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(&real_idx) = state.list_state.selected.and_then(|i| filtered.get(i)) {
                if let Some(dir) = state.entries[real_idx].path.parent() {
                    if let Err(e) = open::that(dir) {
                        tracing::error!("Failed to open directory: {}", e);
                    }
                }
            }
            true
        }
        _ => false,
    }
}

/// Returns `true` if the key was consumed.
pub fn handle_key(key_event: &KeyEvent, state: &mut ContentListState) -> bool {
    if handle_search_keys(key_event, state) {
        return true;
    }
    let filtered = state.filtered_indices();
    let count = filtered.len();
    match key_event.code {
        KeyCode::Char('j') | KeyCode::Down => {
            if count == 0 {
                return true;
            }
            let current = state.list_state.selected.unwrap_or(0);
            state.list_state.selected = Some((current + 1).min(count - 1));
            state.update_scrollbar();
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let current = state.list_state.selected.unwrap_or(0);
            state.list_state.selected = Some(current.saturating_sub(1));
            state.update_scrollbar();
            true
        }
        KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(&real_idx) = state.list_state.selected.and_then(|i| filtered.get(i)) {
                if let Some(dir) = state.entries[real_idx].path.parent() {
                    if let Err(e) = open::that(dir) {
                        tracing::error!("Failed to open directory: {}", e);
                    }
                }
            }
            true
        }
        KeyCode::Enter => {
            if let Some(&real_idx) = state.list_state.selected.and_then(|i| filtered.get(i)) {
                state.list_state.selected = Some(real_idx);
                state.toggle_selected();
                state.list_state.selected =
                    Some(filtered.iter().position(|&i| i == real_idx).unwrap_or(0));
            }
            true
        }
        _ => false,
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    state: &mut ContentListState,
    is_focused: bool,
    loading_text: &str,
    empty_text: &str,
) {
    let theme = THEME.as_ref();
    if state.loading {
        frame.render_widget(
            Paragraph::new(loading_text)
                .style(Style::default().fg(theme.text_dim())),
            area,
        );
        return;
    }

    let filtered = state.filtered_indices();

    if filtered.is_empty() {
        frame.render_widget(
            Paragraph::new(empty_text)
                .style(Style::default().fg(theme.text_dim())),
            area,
        );
        return;
    }

    let count = filtered.len();

    let snapshot: Vec<SnapshotRow> = filtered
        .iter()
        .map(|&i| {
            let entry = &state.entries[i];
            (
                entry.name.clone(),
                entry.description.clone(),
                entry.enabled,
                entry.icon_lines.clone(),
            )
        })
        .collect();

    let builder = ListBuilder::new(move |context| {
        let theme = THEME.as_ref();
        let (name, description, enabled, icon_pixels) = &snapshot[context.index];
        let show_selected = is_focused && context.is_selected;
        let use_mc_colors = *enabled;

        let stripe_bg = if context.index % 2 == 0 {
            theme.background()
        } else {
            theme.stripe()
        };

        let (name_style, description_style, background) = match (*enabled, show_selected) {
            (true, true) => (
                Style::default().fg(theme.accent()).add_modifier(Modifier::BOLD),
                Style::default().fg(theme.text_dim()),
                stripe_bg,
            ),
            (true, false) => (
                Style::default()
                    .fg(theme.text())
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(theme.text_dim()),
                stripe_bg,
            ),
            (false, true) => (
                Style::default().fg(theme.accent()).add_modifier(Modifier::CROSSED_OUT),
                Style::default().fg(theme.text_dim()),
                stripe_bg,
            ),
            (false, false) => (
                Style::default().fg(theme.text_dim()).add_modifier(Modifier::CROSSED_OUT),
                Style::default().fg(theme.text_dim()),
                stripe_bg,
            ),
        };

        let has_icon = icon_pixels.is_some();
        let full_desc = strip_mc_codes(description);
        let stripped_desc = full_desc.lines().next().unwrap_or("").trim().to_string();
        let has_description = !stripped_desc.is_empty();
        let compact = !has_icon && !has_description;

        let selector = if show_selected {
            Span::styled("\u{258c}", Style::default().fg(theme.accent()))
        } else {
            Span::raw(" ")
        };

        if compact {
            let mut line = Vec::new();
            line.push(selector.clone());
            if use_mc_colors {
                line.extend(parse_mc_text(name, name_style));
            } else {
                line.push(Span::styled(strip_mc_codes(name), name_style));
            }

            let item = Text::from(vec![Line::from(line)]).style(Style::default().bg(background));
            (item, 1)
        } else if has_icon {
            let icon_row_count = icon_pixels.as_ref().map(|r| r.len()).unwrap_or(0);
            let text_rows = if has_description { 2 } else { 1 }; // name + optional description
            let height = icon_row_count.max(text_rows) as u16;

            let pad = if show_selected {
                Span::styled("\u{258c}", Style::default().fg(theme.accent()))
            } else {
                Span::raw(" ")
            };

            let mut line_0 = vec![selector.clone()];
            line_0.extend(icon_spans(icon_pixels.as_ref(), 0));
            line_0.push(Span::raw(" "));
            if use_mc_colors {
                line_0.extend(parse_mc_text(name, name_style));
            } else {
                line_0.push(Span::styled(strip_mc_codes(name), name_style));
            }

            let mut lines = vec![Line::from(line_0)];

            if has_description {
                let mut row = vec![pad.clone()];
                row.extend(icon_spans(icon_pixels.as_ref(), 1));
                row.push(Span::raw(" "));
                row.push(Span::styled(stripped_desc.clone(), description_style));
                lines.push(Line::from(row));
            }

            let desc_rows = if has_description { 1 } else { 0 };
            for r in (1 + desc_rows)..icon_row_count {
                let mut row = vec![pad.clone()];
                row.extend(icon_spans(icon_pixels.as_ref(), r));
                lines.push(Line::from(row));
            }

            let item = Text::from(lines).style(Style::default().bg(background));
            (item, height)
        } else {
            let mut line_0 = Vec::new();
            line_0.push(selector.clone());
            if use_mc_colors {
                line_0.extend(parse_mc_text(name, name_style));
            } else {
                line_0.push(Span::styled(strip_mc_codes(name), name_style));
            }

            let mut lines = vec![Line::from(line_0)];

            if has_description {
                let pad = if show_selected {
                    Span::styled("\u{258c}", Style::default().fg(theme.accent()))
                } else {
                    Span::raw(" ")
                };
                lines.push(Line::from(vec![
                    pad,
                    Span::styled(stripped_desc.clone(), description_style),
                ]));
            }

            let height = lines.len() as u16;
            let item = Text::from(lines).style(Style::default().bg(background));
            (item, height)
        }
    });

    let list = ListView::new(builder, count);
    frame.render_stateful_widget(list, area, &mut state.list_state);

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
                    .fg(theme.text_dim())
                    .add_modifier(Modifier::BOLD),
            )
            .thumb_symbol("\u{2551}")
            .track_symbol(Some(""))
            .end_symbol(Some("\u{25bc}")),
        scrollbar_area,
        &mut state.scrollbar_state,
    );
}

fn mc_color(code: char) -> Option<Color> {
    match code {
        '0' => Some(Color::Rgb(0x00, 0x00, 0x00)),
        '1' => Some(Color::Rgb(0x00, 0x00, 0xAA)),
        '2' => Some(Color::Rgb(0x00, 0xAA, 0x00)),
        '3' => Some(Color::Rgb(0x00, 0xAA, 0xAA)),
        '4' => Some(Color::Rgb(0xAA, 0x00, 0x00)),
        '5' => Some(Color::Rgb(0xAA, 0x00, 0xAA)),
        '6' => Some(Color::Rgb(0xFF, 0xAA, 0x00)),
        '7' => Some(Color::Rgb(0xAA, 0xAA, 0xAA)),
        '8' => Some(Color::Rgb(0x55, 0x55, 0x55)),
        '9' => Some(Color::Rgb(0x55, 0x55, 0xFF)),
        'a' | 'A' => Some(Color::Rgb(0x55, 0xFF, 0x55)),
        'b' | 'B' => Some(Color::Rgb(0x55, 0xFF, 0xFF)),
        'c' | 'C' => Some(Color::Rgb(0xFF, 0x55, 0x55)),
        'd' | 'D' => Some(Color::Rgb(0xFF, 0x55, 0xFF)),
        'e' | 'E' => Some(Color::Rgb(0xFF, 0xFF, 0x55)),
        'f' | 'F' => Some(Color::Rgb(0xFF, 0xFF, 0xFF)),
        _ => None,
    }
}

fn parse_mc_text(text: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current_style = base_style;
    let mut current_text = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{00A7}' {
            if let Some(&code) = chars.peek() {
                if !current_text.is_empty() {
                    spans.push(Span::styled(current_text.clone(), current_style));
                    current_text.clear();
                }
                chars.next();

                if let Some(color) = mc_color(code) {
                    current_style = base_style.fg(color);
                } else {
                    match code {
                        'l' | 'L' => {
                            current_style = current_style.add_modifier(Modifier::BOLD);
                        }
                        'm' | 'M' => {
                            current_style = current_style.add_modifier(Modifier::CROSSED_OUT);
                        }
                        'n' | 'N' => {
                            current_style = current_style.add_modifier(Modifier::UNDERLINED);
                        }
                        'o' | 'O' => {
                            current_style = current_style.add_modifier(Modifier::ITALIC);
                        }
                        'r' | 'R' => {
                            current_style = base_style;
                        }
                        _ => {}
                    }
                }
                continue;
            }
        }
        current_text.push(ch);
    }

    if !current_text.is_empty() {
        spans.push(Span::styled(current_text, current_style));
    }

    spans
}

fn strip_mc_codes(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{00A7}' {
            chars.next();
        } else {
            result.push(ch);
        }
    }
    result
}

fn icon_spans(icon_pixels: Option<&Vec<Vec<IconCell>>>, row: usize) -> Vec<Span<'static>> {
    match icon_pixels.and_then(|rows| rows.get(row)) {
        Some(cols) => cols
            .iter()
            .map(|cell| {
                Span::styled(
                    "\u{2584}",
                    Style::default()
                        .fg(Color::Rgb(cell.fg_r, cell.fg_g, cell.fg_b))
                        .bg(Color::Rgb(cell.bg_r, cell.bg_g, cell.bg_b)),
                )
            })
            .collect(),
        None => {
            let theme = THEME.as_ref();
            vec![Span::styled(
                "      ",
                Style::default().fg(theme.text_dim()),
            )]
        }
    }
}
