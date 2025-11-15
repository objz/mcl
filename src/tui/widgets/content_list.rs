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

use crate::instance::mods::ModEntry;
use crate::tui::theme::THEME;

type IconCell = (u8, u8, u8, u8, u8, u8);

struct CachedList {
    entries: Vec<ModEntry>,
    selected: Option<usize>,
}

pub struct ContentListState {
    pub entries: Vec<ModEntry>,
    pub list_state: TuiListState,
    pub scrollbar_state: ScrollbarState,
    pub loaded_for: Option<String>,
    pub loading: bool,
    cache: HashMap<String, CachedList>,
    pending: Arc<Mutex<Option<(String, Vec<ModEntry>)>>>,
}

impl Default for ContentListState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            list_state: TuiListState::default(),
            scrollbar_state: ScrollbarState::default(),
            loaded_for: None,
            loading: false,
            cache: HashMap::new(),
            pending: Arc::new(Mutex::new(None)),
        }
    }
}

impl ContentListState {
    pub fn start_load<F>(&mut self, instances_dir: &Path, instance_name: &str, scan_fn: F)
    where
        F: FnOnce(&Path, &str) -> Vec<ModEntry> + Send + 'static,
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

pub fn handle_key_no_toggle(key_event: &KeyEvent, state: &mut ContentListState) -> bool {
    match key_event.code {
        KeyCode::Char('j') | KeyCode::Down => {
            let count = state.entries.len();
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
            if let Some(entry) = state
                .list_state
                .selected
                .and_then(|i| state.entries.get(i))
            {
                if let Some(dir) = entry.path.parent() {
                    if let Err(e) = std::process::Command::new("xdg-open")
                        .arg(dir)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                    {
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
    match key_event.code {
        KeyCode::Char('j') | KeyCode::Down => {
            let count = state.entries.len();
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
            if let Some(entry) = state
                .list_state
                .selected
                .and_then(|i| state.entries.get(i))
            {
                if let Some(dir) = entry.path.parent() {
                    if let Err(e) = std::process::Command::new("xdg-open")
                        .arg(dir)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                    {
                        tracing::error!("Failed to open directory: {}", e);
                    }
                }
            }
            true
        }
        KeyCode::Enter => {
            state.toggle_selected();
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
    if state.loading {
        frame.render_widget(
            Paragraph::new(loading_text).style(Style::default().fg(THEME.colors.text_idle)),
            area,
        );
        return;
    }

    if state.entries.is_empty() {
        frame.render_widget(
            Paragraph::new(empty_text).style(Style::default().fg(THEME.colors.text_idle)),
            area,
        );
        return;
    }

    let count = state.entries.len();

    let snapshot: Vec<(String, String, bool, Option<Vec<Vec<IconCell>>>)> = state
        .entries
        .iter()
        .map(|entry| {
            (
                entry.name.clone(),
                entry.description.clone(),
                entry.enabled,
                entry.icon_lines.clone(),
            )
        })
        .collect();

    let builder = ListBuilder::new(move |context| {
        let (name, description, enabled, icon_pixels) = &snapshot[context.index];
        let show_selected = is_focused && context.is_selected;
        let use_mc_colors = *enabled && !show_selected;

        let (name_style, description_style, background) = match (*enabled, show_selected) {
            (true, true) => (
                Style::default()
                    .fg(THEME.colors.row_highlight)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(THEME.colors.row_highlight),
                THEME.colors.row_background,
            ),
            (true, false) => (
                Style::default()
                    .fg(THEME.colors.foreground)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(THEME.colors.text_idle),
                THEME.colors.row_alternate_bg,
            ),
            (false, true) => (
                Style::default()
                    .fg(THEME.colors.row_highlight)
                    .add_modifier(Modifier::CROSSED_OUT),
                Style::default().fg(THEME.colors.row_highlight),
                THEME.colors.row_background,
            ),
            (false, false) => (
                Style::default()
                    .fg(THEME.colors.text_idle)
                    .add_modifier(Modifier::CROSSED_OUT),
                Style::default().fg(THEME.colors.text_idle),
                THEME.colors.row_alternate_bg,
            ),
        };

        let has_icon = icon_pixels.is_some();
        let stripped_desc = strip_mc_codes(description);
        let has_description = !stripped_desc.trim().is_empty();
        let compact = !has_icon && !has_description;

        if compact {
            let mut line = Vec::new();
            line.push(Span::raw(" "));
            if use_mc_colors {
                line.extend(parse_mc_text(name, name_style));
            } else {
                line.push(Span::styled(strip_mc_codes(name), name_style));
            }

            let item = Text::from(vec![Line::from(line)]).style(Style::default().bg(background));
            (item, 1)
        } else if has_icon {
            let icon_row_count = icon_pixels.as_ref().map(|r| r.len()).unwrap_or(0);
            let height = icon_row_count.max(2) as u16;

            let mut line_0 = icon_spans(icon_pixels.as_ref(), 0);
            line_0.push(Span::raw(" "));
            if use_mc_colors {
                line_0.extend(parse_mc_text(name, name_style));
            } else {
                line_0.push(Span::styled(strip_mc_codes(name), name_style));
            }

            let mut line_1 = icon_spans(icon_pixels.as_ref(), 1);
            line_1.push(Span::raw(" "));
            if use_mc_colors {
                line_1.extend(parse_mc_text(description, description_style));
            } else {
                line_1.push(Span::styled(stripped_desc, description_style));
            }

            let mut lines = vec![Line::from(line_0), Line::from(line_1)];
            for r in 2..icon_row_count {
                lines.push(Line::from(icon_spans(icon_pixels.as_ref(), r)));
            }

            let item = Text::from(lines).style(Style::default().bg(background));
            (item, height)
        } else {
            let mut line_0 = Vec::new();
            line_0.push(Span::raw(" "));
            if use_mc_colors {
                line_0.extend(parse_mc_text(name, name_style));
            } else {
                line_0.push(Span::styled(strip_mc_codes(name), name_style));
            }

            let mut line_1 = Vec::new();
            line_1.push(Span::raw(" "));
            if use_mc_colors {
                line_1.extend(parse_mc_text(description, description_style));
            } else {
                line_1.push(Span::styled(stripped_desc, description_style));
            }

            let item = Text::from(vec![Line::from(line_0), Line::from(line_1)])
                .style(Style::default().bg(background));
            (item, 2)
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
            .map(|&(fg_r, fg_g, fg_b, bg_r, bg_g, bg_b)| {
                Span::styled(
                    "\u{2584}",
                    Style::default()
                        .fg(Color::Rgb(fg_r, fg_g, fg_b))
                        .bg(Color::Rgb(bg_r, bg_g, bg_b)),
                )
            })
            .collect(),
        None => vec![Span::styled(
            "      ",
            Style::default().fg(THEME.colors.text_idle),
        )],
    }
}
