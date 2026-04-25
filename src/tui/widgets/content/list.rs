// generic scrollable list for content items (mods, resource packs, shaders, worlds).
// supports toggling items on/off by renaming files with .disabled suffix,
// search filtering, per-instance caching, and directory change detection.
// also handles minecraft's formatting codes for colored mod names/descriptions
// because apparently mojang thought terminal UIs would need that. thanks guys

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use tui_widget_list::{ListBuilder, ListState as TuiListState, ListView};

use crate::config::theme::THEME;
use crate::instance::content::mods::{ContentEntry, IconCell};

type SnapshotRow = (String, String, bool, Option<Vec<Vec<IconCell>>>);
type ScanOneFn = fn(&Path, &str, bool) -> ContentEntry;

struct CachedList {
    entries: Vec<ContentEntry>,
    selected: Option<usize>,
}

// result from the notify-triggered background diff
struct WatcherDiff {
    toggled: Vec<(String, bool, std::path::PathBuf)>,
    removed: Vec<String>,
    added: Vec<ContentEntry>,
}

pub struct ContentListState {
    pub entries: Vec<ContentEntry>,
    pub list_state: TuiListState,
    pub scrollbar_state: ScrollbarState,
    pub loaded_for: Option<String>,
    pub loading: bool,
    pub search: crate::tui::widgets::search::SearchState,
    cache: HashMap<String, CachedList>,
    // streaming: individual entries arrive here during initial load
    stream_rx: Option<mpsc::Receiver<ContentEntry>>,
    // file watcher: notify callback spawns background work,
    // precomputed diff lands here for the UI to pick up
    watcher_diff: Arc<Mutex<Option<WatcherDiff>>>,
    _watcher: Option<notify::RecommendedWatcher>,
    watched_dir: Option<std::path::PathBuf>,
    // stored for the watcher to scan individual new files
    scan_one_fn: Option<ScanOneFn>,
    content_ext: Option<&'static str>,
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
            stream_rx: None,
            watcher_diff: Arc::new(Mutex::new(None)),
            _watcher: None,
            watched_dir: None,
            scan_one_fn: None,
            content_ext: None,
        }
    }
}

impl ContentListState {
    // drain streaming entries from the initial load. each entry arrives
    // individually and is inserted in sorted position for a smooth fill-in
    pub fn drain_pending(&mut self) {
        let Some(rx) = &self.stream_rx else {
            return;
        };

        let mut received = false;
        let mut finished = false;
        loop {
            match rx.try_recv() {
                Ok(entry) => {
                    received = true;
                    let pos = self
                        .entries
                        .binary_search_by(|e| {
                            e.name.to_lowercase().cmp(&entry.name.to_lowercase())
                        })
                        .unwrap_or_else(|i| i);
                    self.entries.insert(pos, entry);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.stream_rx = None;
                    finished = true;
                    break;
                }
            }
        }

        if received || finished {
            self.loading = false;
            if self.list_state.selected.is_none() && !self.entries.is_empty() {
                self.list_state.selected = Some(0);
            }
            self.update_scrollbar();
        }
    }

    // pick up the precomputed diff from the notify watcher callback.
    // skip while streaming is in progress to avoid duplicate entries.
    pub fn drain_watcher(&mut self) {
        if self.stream_rx.is_some() {
            return;
        }

        let diff = match self.watcher_diff.lock() {
            Ok(mut slot) => slot.take(),
            _ => None,
        };

        let Some(diff) = diff else {
            return;
        };

        // apply toggles (enabled/path changes)
        for (stem, enabled, path) in &diff.toggled {
            if let Some(entry) = self.entries.iter_mut().find(|e| &e.file_stem == stem) {
                entry.enabled = *enabled;
                entry.path = path.clone();
            }
        }

        // apply removals
        if !diff.removed.is_empty() {
            self.entries
                .retain(|e| !diff.removed.contains(&e.file_stem));
        }

        // insert new entries in sorted position
        for entry in diff.added {
            let pos = self
                .entries
                .binary_search_by(|e| e.name.to_lowercase().cmp(&entry.name.to_lowercase()))
                .unwrap_or_else(|i| i);
            self.entries.insert(pos, entry);
        }

        // clamp selected
        if let Some(sel) = self.list_state.selected {
            if self.entries.is_empty() {
                self.list_state.selected = None;
            } else {
                self.list_state.selected =
                    Some(sel.min(self.entries.len().saturating_sub(1)));
            }
        }

        self.update_scrollbar();
    }

    // starts a notify file watcher on the given directory. changes trigger
    // a background diff that lands in watcher_diff for drain_watcher to apply.
    pub fn watch_dir(&mut self, dir: std::path::PathBuf) {
        use notify::{RecursiveMode, Watcher};
        use std::sync::atomic::{AtomicBool, Ordering};

        // drop previous watcher
        self._watcher = None;

        let watcher_diff = self.watcher_diff.clone();
        let ext: &'static str = self.content_ext.unwrap_or(".jar");
        let scan_one = self.scan_one_fn;

        let dirty = Arc::new(AtomicBool::new(false));
        let running = Arc::new(AtomicBool::new(false));
        let dirty_cb = dirty.clone();
        let running_cb = running.clone();

        // initialize known stems from the current directory state so existing
        // files are not treated as "new" on the first notify event
        let known_stems = Arc::new(Mutex::new(read_dir_stems(&dir, ext)));

        let watch_dir = dir.clone();
        let watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
            if res.is_err() {
                return;
            }

            // mark dirty. if a thread is already running it will loop to
            // pick up the change after its current diff
            dirty_cb.store(true, Ordering::Relaxed);

            if running_cb.swap(true, Ordering::Relaxed) {
                return;
            }

            let dir = watch_dir.clone();
            let diff_slot = watcher_diff.clone();
            let dirty = dirty_cb.clone();
            let running = running_cb.clone();
            let known = known_stems.clone();

            std::thread::spawn(move || {
                // always clear `running` even if we panic
                struct ResetOnDrop(Arc<AtomicBool>);
                impl Drop for ResetOnDrop {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::Relaxed);
                    }
                }
                let _guard = ResetOnDrop(running);

                loop {
                    dirty.store(false, Ordering::Relaxed);
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    let result = (|| {
                        let on_disk = read_dir_stems(&dir, ext);
                        let mut known_map = known.lock().ok()?;

                        let mut toggled = Vec::new();
                        let mut removed = Vec::new();
                        let mut added = Vec::new();

                        for (stem, (old_path, old_enabled)) in known_map.iter() {
                            if let Some((disk_path, disk_enabled)) = on_disk.get(stem) {
                                if *disk_enabled != *old_enabled || *disk_path != *old_path {
                                    toggled.push((
                                        stem.clone(),
                                        *disk_enabled,
                                        disk_path.clone(),
                                    ));
                                }
                            } else {
                                removed.push(stem.clone());
                            }
                        }

                        for (stem, (path, enabled)) in &on_disk {
                            if !known_map.contains_key(stem)
                                && let Some(scan_one) = scan_one
                            {
                                added.push(scan_one(path, stem, *enabled));
                            }
                        }

                        *known_map = on_disk;

                        Some(WatcherDiff {
                            toggled,
                            removed,
                            added,
                        })
                    })();

                    if let Some(diff) = result
                        && let Ok(mut slot) = diff_slot.lock()
                    {
                        *slot = Some(diff);
                    }

                    if !dirty.load(Ordering::Relaxed) {
                        break;
                    }
                }
            });
        });

        match watcher {
            Ok(mut w) => {
                if let Err(e) = w.watch(&dir, RecursiveMode::NonRecursive) {
                    tracing::warn!("Failed to watch {}: {e}", dir.display());
                } else {
                    self._watcher = Some(w);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to create file watcher: {e}");
            }
        }

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
    // saves current entries to cache before loading new ones, and restores
    // from cache if this instance was seen before (avoids re-scanning).
    // content_dir is the actual directory to scan (e.g. .minecraft/mods).
    pub fn start_load(
        &mut self,
        content_dir: &Path,
        instance_name: &str,
        scan_one_fn: ScanOneFn,
        ext: &'static str,
    ) {
        self.scan_one_fn = Some(scan_one_fn);
        self.content_ext = Some(ext);

        // save current entries to cache
        if let Some(prev) = self.loaded_for.take()
            && !self.entries.is_empty()
        {
            self.cache.insert(
                prev,
                CachedList {
                    entries: std::mem::take(&mut self.entries),
                    selected: self.list_state.selected,
                },
            );
        }

        // try cache first
        if let Some(cached) = self.cache.remove(instance_name) {
            self.entries = cached.entries;
            self.list_state.selected = cached.selected;
            self.loading = false;
            self.stream_rx = None;
            self.loaded_for = Some(instance_name.to_string());
            self.update_scrollbar();
            return;
        }

        // no cache, stream entries one by one as each file is scanned
        self.entries.clear();
        self.list_state = TuiListState::default();
        self.loading = true;
        self.loaded_for = Some(instance_name.to_string());
        self.update_scrollbar();

        let (tx, rx) = mpsc::channel();
        self.stream_rx = Some(rx);

        let dir = content_dir.to_path_buf();

        tokio::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                let read_dir = match std::fs::read_dir(&dir) {
                    Ok(rd) => rd,
                    Err(_) => return,
                };
                let disabled_ext = format!("{ext}.disabled");

                for dir_entry in read_dir.flatten() {
                    let path = dir_entry.path();
                    let Some(fname) = path.file_name().and_then(|n| n.to_str()) else {
                        continue;
                    };

                    let (enabled, file_stem) =
                        if let Some(stem) = fname.strip_suffix(&disabled_ext) {
                            (false, stem.to_owned())
                        } else if let Some(stem) = fname.strip_suffix(ext) {
                            (true, stem.to_owned())
                        } else if path.is_dir() {
                            crate::instance::content::parse_enabled_stem_dir(fname)
                        } else {
                            continue;
                        };

                    let entry = scan_one_fn(&path, &file_stem, enabled);
                    if tx.send(entry).is_err() {
                        break; // receiver dropped (instance switched)
                    }
                }
            })
            .await;
        });
    }

    fn update_scrollbar(&mut self) {
        let count = self.entries.len();
        let max = count.saturating_sub(1);
        let pos = self.list_state.selected.unwrap_or(0);
        self.scrollbar_state = ScrollbarState::new(max).position(pos);
    }

    // enable/disable by renaming the file with/without .disabled extension.
    // this is how most minecraft launchers handle it
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
            KeyCode::Enter => {
                state.search.confirm();
                state.list_state.selected = Some(0);
                state.update_scrollbar();
            }
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
            if let Some(&real_idx) = state.list_state.selected.and_then(|i| filtered.get(i))
                && let Some(dir) = state.entries[real_idx].path.parent()
                && let Err(e) = open::that(dir)
            {
                tracing::error!("Failed to open directory: {}", e);
            }
            true
        }
        _ => false,
    }
}

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
            if let Some(&real_idx) = state.list_state.selected.and_then(|i| filtered.get(i))
                && let Some(dir) = state.entries[real_idx].path.parent()
                && let Err(e) = open::that(dir)
            {
                tracing::error!("Failed to open directory: {}", e);
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
            Paragraph::new(loading_text).style(Style::default().fg(theme.text_dim())),
            area,
        );
        return;
    }

    let filtered = state.filtered_indices();

    if filtered.is_empty() {
        state.list_state.selected = None;
        frame.render_widget(
            Paragraph::new(empty_text).style(Style::default().fg(theme.text_dim())),
            area,
        );
        return;
    }

    let count = filtered.len();

    // clamp selected so the ListView builder never gets an out-of-bounds index
    if let Some(sel) = state.list_state.selected
        && sel >= count
    {
        state.list_state.selected = Some(count.saturating_sub(1));
    }

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
                Style::default()
                    .fg(theme.accent())
                    .add_modifier(Modifier::BOLD),
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
                Style::default()
                    .fg(theme.accent())
                    .add_modifier(Modifier::CROSSED_OUT),
                Style::default().fg(theme.text_dim()),
                stripe_bg,
            ),
            (false, false) => (
                Style::default()
                    .fg(theme.text_dim())
                    .add_modifier(Modifier::CROSSED_OUT),
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

// minecraft's 16-color palette, keyed by the formatting code character.
// these exact RGB values come from the minecraft wiki
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

// parses minecraft's section-sign (U+00A7) formatting codes into styled spans.
// handles colors (0-f), bold (l), strikethrough (m), underline (n), italic (o), reset (r)
fn parse_mc_text(text: &str, base_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut current_style = base_style;
    let mut current_text = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{00A7}'
            && let Some(&code) = chars.peek()
        {
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

// renders one row of a mod icon using half-block characters (U+2584).
// each cell packs two vertical pixels via fg/bg colors, giving
// double the vertical resolution out of the terminal
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

// reads a content directory and builds a stem -> (path, enabled) map.
// used both by watch_dir to initialize known state and by the watcher
// thread to detect changes. when ext is empty (worlds), only directories
// are included.
fn read_dir_stems(
    dir: &std::path::Path,
    ext: &str,
) -> HashMap<String, (std::path::PathBuf, bool)> {
    let mut map = HashMap::new();
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return map;
    };
    let dirs_only = ext.is_empty();
    let disabled_ext = format!("{ext}.disabled");

    for dir_entry in read_dir.flatten() {
        let path = dir_entry.path();
        let Some(fname) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if dirs_only {
            if !path.is_dir() && !fname.ends_with(".disabled") {
                continue;
            }
            let (enabled, stem) = crate::instance::content::parse_enabled_stem_dir(fname);
            map.insert(stem, (path, enabled));
            continue;
        }
        if let Some(stem) = fname.strip_suffix(&disabled_ext) {
            map.insert(stem.to_owned(), (path, false));
        } else if let Some(stem) = fname.strip_suffix(ext) {
            map.insert(stem.to_owned(), (path, true));
        } else if path.is_dir() {
            let (enabled, stem) = crate::instance::content::parse_enabled_stem_dir(fname);
            map.insert(stem, (path, enabled));
        }
    }

    map
}
