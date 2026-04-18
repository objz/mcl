use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};
use ratatui_image::{protocol::StatefulProtocol, Resize, StatefulImage};

use crate::instance::screenshots::ScreenshotEntry;
use crate::config::theme::THEME;

const TARGET_CELL_WIDTH: u16 = 34;
const MIN_CELL_WIDTH: u16 = 24;
const MAX_CELL_WIDTH: u16 = 52;
const NAME_ROW_HEIGHT: u16 = 1;
const GAP: u16 = 1;

type PendingScreenshots = Arc<Mutex<Option<(String, Vec<ScreenshotEntry>)>>>;

pub struct ScreenshotsState {
    pub entries: Vec<ScreenshotEntry>,
    protocols: HashMap<usize, StatefulProtocol>,
    requested: HashSet<usize>,
    pub selected: usize,
    pub scroll_row: usize,
    pub loaded_for: Option<String>,
    pub loading: bool,
    cols: usize,
    visible_rows: usize,
    pub scrollbar_state: ScrollbarState,
    pub search: super::search::SearchState,
    pub font_size: (u16, u16),
    pending_entries: PendingScreenshots,
    pending_images: Arc<Mutex<Vec<(usize, image::DynamicImage)>>>,
}

impl Default for ScreenshotsState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            protocols: HashMap::new(),
            requested: HashSet::new(),
            selected: 0,
            scroll_row: 0,
            loaded_for: None,
            loading: false,
            cols: 3,
            visible_rows: 2,
            scrollbar_state: ScrollbarState::default(),
            search: super::search::SearchState::default(),
            font_size: (8, 16),
            pending_entries: Arc::new(Mutex::new(None)),
            pending_images: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl ScreenshotsState {
    pub fn start_load(&mut self, instances_dir: &Path, instance_name: &str) {
        self.loading = true;
        self.loaded_for = Some(instance_name.to_string());
        self.entries.clear();
        self.protocols.clear();
        self.requested.clear();
        self.selected = 0;
        self.scroll_row = 0;

        let dir = instances_dir.to_path_buf();
        let tag = instance_name.to_string();
        let pending = self.pending_entries.clone();

        tokio::spawn(async move {
            let scan_dir = dir.clone();
            let scan_name = tag.clone();
            let entries = tokio::task::spawn_blocking(move || {
                crate::instance::screenshots::scan_screenshots(&scan_dir, &scan_name)
            })
            .await
            .unwrap_or_default();

            if let Ok(mut slot) = pending.lock() {
                *slot = Some((tag, entries));
            }
        });
    }

    pub fn drain_pending_entries(&mut self) {
        let taken = if let Ok(mut slot) = self.pending_entries.lock() {
            slot.take()
        } else {
            None
        };

        if let Some((instance_name, entries)) = taken {
            if self.loaded_for.as_deref() == Some(&instance_name) {
                self.entries = entries;
                self.loading = false;
                self.selected = 0;
                self.scroll_row = 0;
            }
        }
    }

    pub fn take_pending_images(&mut self) -> Vec<(usize, image::DynamicImage)> {
        if let Ok(mut slot) = self.pending_images.lock() {
            std::mem::take(&mut *slot)
        } else {
            Vec::new()
        }
    }

    pub fn set_protocol(&mut self, idx: usize, proto: StatefulProtocol) {
        self.protocols.insert(idx, proto);
    }

    pub fn request_visible_loads(&mut self) {
        if self.entries.is_empty() {
            return;
        }

        let first = self.scroll_row * self.cols;
        let last = ((self.scroll_row + self.visible_rows + 1) * self.cols).min(self.entries.len());

        for idx in first..last {
            if !self.protocols.contains_key(&idx) && self.requested.insert(idx) {
                let path = self.entries[idx].path.clone();
                let pending = self.pending_images.clone();

                tokio::spawn(async move {
                    let load_path = path.clone();
                    let img = tokio::task::spawn_blocking(move || image::open(&load_path).ok())
                        .await
                        .unwrap_or(None);

                    if let Some(img) = img {
                        if let Ok(mut slot) = pending.lock() {
                            slot.push((idx, img));
                        }
                    }
                });
            }
        }
    }

    fn ensure_visible(&mut self) {
        let row = if self.cols > 0 {
            self.selected / self.cols
        } else {
            0
        };

        if row < self.scroll_row {
            self.scroll_row = row;
        } else if row >= self.scroll_row + self.visible_rows {
            self.scroll_row = row.saturating_sub(self.visible_rows - 1);
        }

        let total = self.total_rows().saturating_sub(1);
        self.scrollbar_state = ScrollbarState::new(total).position(self.scroll_row);
    }

    fn total_rows(&self) -> usize {
        if self.cols == 0 {
            return 0;
        }
        self.entries.len().div_ceil(self.cols)
    }
}

pub fn handle_key(key_event: &KeyEvent, state: &mut ScreenshotsState) -> bool {
    if state.search.active {
        match key_event.code {
            KeyCode::Esc => {
                state.search.deactivate();
                state.selected = 0;
            }
            KeyCode::Backspace => {
                state.search.pop();
                state.selected = 0;
            }
            KeyCode::Char(c) => {
                state.search.push(c);
                state.selected = 0;
            }
            _ => {}
        }
        return true;
    }

    let filtered: Vec<usize> = state
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| state.search.matches(&e.name))
        .map(|(i, _)| i)
        .collect();
    let count = filtered.len();
    if count == 0 {
        if key_event.code == KeyCode::Char('/') {
            state.search.activate();
            return true;
        }
        return false;
    }
    let cols = state.cols.max(1);

    match key_event.code {
        KeyCode::Char('/') => {
            state.search.activate();
            state.selected = 0;
            true
        }
        KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(entry) = state.entries.get(state.selected) {
                if let Some(dir) = entry.path.parent() {
                    if let Err(e) = open::that(dir) {
                        tracing::error!("Failed to open directory: {}", e);
                    }
                }
            }
            true
        }
        KeyCode::Enter => {
            if let Some(entry) = state.entries.get(state.selected) {
                if let Err(e) = open::that(&entry.path) {
                    tracing::error!("Failed to open file: {}", e);
                }
            }
            true
        }
        KeyCode::Char('L') | KeyCode::Right
            if key_event.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            if state.selected + 1 < count {
                state.selected += 1;
                state.ensure_visible();
            }
            true
        }
        KeyCode::Char('H') | KeyCode::Left if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
            state.selected = state.selected.saturating_sub(1);
            state.ensure_visible();
            true
        }
        KeyCode::Char('J') | KeyCode::Down if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
            let next = state.selected + cols;
            if next < count {
                state.selected = next;
            }
            state.ensure_visible();
            true
        }
        KeyCode::Char('K') | KeyCode::Up if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
            state.selected = state.selected.saturating_sub(cols);
            state.ensure_visible();
            true
        }
        _ => false,
    }
}

pub fn render(frame: &mut Frame, area: Rect, state: &mut ScreenshotsState, is_focused: bool) {
    let theme = THEME.as_ref();
    if state.loading {
        frame.render_widget(
            Paragraph::new("Loading screenshots...")
                .style(Style::default().fg(theme.text_dim())),
            area,
        );
        return;
    }

    if state.entries.is_empty() {
        frame.render_widget(
            Paragraph::new("No screenshots.")
                .style(Style::default().fg(theme.text_dim())),
            area,
        );
        return;
    }

    let min_cols = (area.width / MAX_CELL_WIDTH).max(1) as usize;
    let max_cols = (area.width / MIN_CELL_WIDTH).max(1) as usize;
    let target_cols = (area.width / TARGET_CELL_WIDTH).max(1) as usize;
    let cols = target_cols.clamp(min_cols, max_cols);
    let cell_width = area.width / cols as u16;

    let (img_w, img_h) = state
        .entries
        .first()
        .map(|e| (e.width, e.height))
        .unwrap_or((1920, 1080));
    let (fw, fh) = (
        state.font_size.0.max(1) as u32,
        state.font_size.1.max(1) as u32,
    );
    let img_rows = (cell_width as u32 * fw * img_h / (fh * img_w)).max(2) as u16;
    let cell_height = img_rows + NAME_ROW_HEIGHT + GAP;
    let visible_rows = (area.height / cell_height).max(1) as usize;

    state.cols = cols;
    state.visible_rows = visible_rows;
    state.ensure_visible();

    for vr in 0..visible_rows {
        for vc in 0..cols {
            let idx = (state.scroll_row + vr) * cols + vc;
            if idx >= state.entries.len() {
                break;
            }

            let raw_x = area.x + vc as u16 * cell_width;
            let raw_y = area.y + vr as u16 * cell_height;
            let raw_w = cell_width.min(area.x + area.width - raw_x);
            let raw_h = cell_height.min(area.y + area.height - raw_y);

            let cell = Rect {
                x: raw_x,
                y: raw_y,
                width: raw_w.saturating_sub(GAP),
                height: raw_h.saturating_sub(GAP),
            };

            if cell.height < 2 || cell.width < 4 {
                continue;
            }

            let is_selected = is_focused && idx == state.selected;

            let [img_area, name_area] =
                Layout::vertical([Constraint::Min(0), Constraint::Length(NAME_ROW_HEIGHT)])
                    .areas(cell);

            if let Some(proto) = state.protocols.get_mut(&idx) {
                let widget: StatefulImage<StatefulProtocol> =
                    StatefulImage::default().resize(Resize::Fit(None));
                frame.render_stateful_widget(widget, img_area, proto);
            }

            let name = &state.entries[idx].name;
            let name_style = if is_selected {
                Style::default()
                    .fg(theme.accent())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text_dim())
            };

            let truncated = if name.len() > cell_width as usize {
                &name[..cell_width as usize]
            } else {
                name
            };
            frame.render_widget(
                Paragraph::new(Span::styled(truncated, name_style)),
                name_area,
            );
        }
    }

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
