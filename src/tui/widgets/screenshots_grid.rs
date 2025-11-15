use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::Paragraph,
    Frame,
};
use ratatui_image::{protocol::StatefulProtocol, Resize, StatefulImage};

use crate::instance::screenshots::ScreenshotEntry;
use crate::tui::theme::THEME;

const TARGET_CELL_WIDTH: u16 = 34;
const MIN_CELL_WIDTH: u16 = 24;
const MAX_CELL_WIDTH: u16 = 52;
const NAME_ROW_HEIGHT: u16 = 1;
const GAP: u16 = 1;

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
    pub font_size: (u16, u16),
    pending_entries: Arc<Mutex<Option<(String, Vec<ScreenshotEntry>)>>>,
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
    }
}

pub fn handle_key(key_event: &KeyEvent, state: &mut ScreenshotsState) -> bool {
    let count = state.entries.len();
    if count == 0 {
        return false;
    }
    let cols = state.cols.max(1);

    match key_event.code {
        KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::SHIFT) => {
            if let Some(entry) = state.entries.get(state.selected) {
                if let Some(dir) = entry.path.parent() {
                    xdg_open(dir);
                }
            }
            true
        }
        KeyCode::Enter => {
            if let Some(entry) = state.entries.get(state.selected) {
                xdg_open(&entry.path);
            }
            true
        }
        KeyCode::Char('l') | KeyCode::Right => {
            if state.selected + 1 < count {
                state.selected += 1;
                state.ensure_visible();
            }
            true
        }
        KeyCode::Char('h') | KeyCode::Left => {
            state.selected = state.selected.saturating_sub(1);
            state.ensure_visible();
            true
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let next = state.selected + cols;
            if next < count {
                state.selected = next;
            }
            state.ensure_visible();
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.selected = state.selected.saturating_sub(cols);
            state.ensure_visible();
            true
        }
        _ => false,
    }
}

fn xdg_open(path: &std::path::Path) {
    if let Err(e) = std::process::Command::new("xdg-open")
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        tracing::error!("Failed to open: {}", e);
    }
}

pub fn render(frame: &mut Frame, area: Rect, state: &mut ScreenshotsState, is_focused: bool) {
    if state.loading {
        frame.render_widget(
            Paragraph::new("Loading screenshots...")
                .style(Style::default().fg(THEME.colors.text_idle)),
            area,
        );
        return;
    }

    if state.entries.is_empty() {
        frame.render_widget(
            Paragraph::new("No screenshots.").style(Style::default().fg(THEME.colors.text_idle)),
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
    let (fw, fh) = (state.font_size.0.max(1) as u32, state.font_size.1.max(1) as u32);
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

            let [img_area, name_area] = Layout::vertical([
                Constraint::Min(0),
                Constraint::Length(NAME_ROW_HEIGHT),
            ])
            .areas(cell);

            if let Some(proto) = state.protocols.get_mut(&idx) {
                let widget: StatefulImage<StatefulProtocol> =
                    StatefulImage::default().resize(Resize::Fit(None));
                frame.render_stateful_widget(widget, img_area, proto);
            }

            let name = &state.entries[idx].name;
            let name_style = if is_selected {
                Style::default()
                    .fg(THEME.colors.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(THEME.colors.text_idle)
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
}
