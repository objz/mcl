// generic scrollable list for content items (mods, resource packs, shaders, worlds).
// supports toggling items on/off by renaming files with .disabled suffix,
// search filtering, per-instance caching, and directory change detection.
// also handles minecraft's formatting codes for colored mod names/descriptions
// because apparently mojang thought terminal UIs would need that. thanks guys

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex, mpsc};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use ratatui_image::{CropOptions, Resize, StatefulImage, protocol::StatefulProtocol};
use tui_widget_list::{ListBuilder, ListState as TuiListState, ListView};

use crate::config::theme::THEME;
use crate::instance::content::mods::{ContentEntry, IconCell};

type ScanOneFn = fn(&Path, &str, bool) -> ContentEntry;
static PROVIDER_ICON_SLOTS: LazyLock<Arc<tokio::sync::Semaphore>> =
    LazyLock::new(|| Arc::new(tokio::sync::Semaphore::new(4)));

#[derive(Clone, Copy, Default)]
enum ContentStreamOrder {
    #[default]
    Sorted,
    Source,
}

#[derive(Clone)]
pub struct ContentStream {
    sender: mpsc::Sender<ContentStreamUpdate>,
}

impl ContentStream {
    pub fn send(&self, entry: ContentEntry) -> bool {
        if self.sender.send(ContentStreamUpdate::Entry(entry)).is_ok() {
            crate::tui::request_redraw();
            true
        } else {
            false
        }
    }

    pub fn send_icon(&self, file_stem: String, path: std::path::PathBuf, bytes: Vec<u8>) -> bool {
        if self
            .sender
            .send(ContentStreamUpdate::Icon {
                file_stem,
                path,
                bytes,
            })
            .is_ok()
        {
            crate::tui::request_redraw();
            true
        } else {
            false
        }
    }

    pub fn send_icon_unavailable(&self, file_stem: String, path: std::path::PathBuf) -> bool {
        if self
            .sender
            .send(ContentStreamUpdate::IconUnavailable { file_stem, path })
            .is_ok()
        {
            crate::tui::request_redraw();
            true
        } else {
            false
        }
    }

    pub fn upsert(&self, entry: ContentEntry) -> bool {
        if self.sender.send(ContentStreamUpdate::Upsert(entry)).is_ok() {
            crate::tui::request_redraw();
            true
        } else {
            false
        }
    }

    pub fn retain(&self, file_stems: HashSet<String>) -> bool {
        if self
            .sender
            .send(ContentStreamUpdate::Retain(file_stems))
            .is_ok()
        {
            crate::tui::request_redraw();
            true
        } else {
            false
        }
    }
}

enum ContentStreamUpdate {
    Entry(ContentEntry),
    Upsert(ContentEntry),
    Retain(HashSet<String>),
    Icon {
        file_stem: String,
        path: std::path::PathBuf,
        bytes: Vec<u8>,
    },
    IconUnavailable {
        file_stem: String,
        path: std::path::PathBuf,
    },
}

struct CachedList {
    entries: Vec<ContentEntry>,
    selected: Option<usize>,
}

struct PendingContentImage {
    file_stem: String,
    path: std::path::PathBuf,
    icon_lines: Vec<Vec<IconCell>>,
    image: Option<image::DynamicImage>,
}

struct PendingProviderIcon {
    provider: String,
    project_id: String,
    bytes: Vec<u8>,
}

struct DisplayMetadata {
    description: String,
    has_description: bool,
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
    image_protocols: HashMap<String, StatefulProtocol>,
    requested_images: HashSet<String>,
    pending_entry_images: HashSet<String>,
    pending_images: Arc<Mutex<Vec<PendingContentImage>>>,
    pending_provider_icons: Arc<Mutex<Vec<PendingProviderIcon>>>,
    requested_provider_icons: HashSet<(String, String)>,
    provider_icon_meta_dir: Option<std::path::PathBuf>,
    provider_icon_client: Option<crate::net::HttpClient>,
    images_dirty: bool,
    display_metadata: HashMap<String, DisplayMetadata>,
    pub search: crate::tui::widgets::search::SearchState,
    filter_search: bool,
    cache: HashMap<String, CachedList>,
    // streaming: individual entries arrive here during initial load
    stream_rx: Option<mpsc::Receiver<ContentStreamUpdate>>,
    stream_order: ContentStreamOrder,
    // file watcher: notify callback spawns background work,
    // precomputed diff lands here for the UI to pick up
    watcher_diff: Arc<Mutex<Option<WatcherDiff>>>,
    _watcher: Option<notify::RecommendedWatcher>,
    watched_dir: Option<std::path::PathBuf>,
    // stored for the watcher to scan individual new files
    scan_one_fn: Option<ScanOneFn>,
    content_ext: Option<&'static str>,
}

#[derive(Clone, Debug)]
pub struct PendingContentDelete {
    pub name: String,
    pub path: std::path::PathBuf,
}

impl Default for ContentListState {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            list_state: TuiListState::default(),
            scrollbar_state: ScrollbarState::default(),
            loaded_for: None,
            loading: false,
            image_protocols: HashMap::new(),
            requested_images: HashSet::new(),
            pending_entry_images: HashSet::new(),
            pending_images: Arc::new(Mutex::new(Vec::new())),
            pending_provider_icons: Arc::new(Mutex::new(Vec::new())),
            requested_provider_icons: HashSet::new(),
            provider_icon_meta_dir: None,
            provider_icon_client: None,
            images_dirty: true,
            display_metadata: HashMap::new(),
            search: crate::tui::widgets::search::SearchState::default(),
            filter_search: true,
            cache: HashMap::new(),
            stream_rx: None,
            stream_order: ContentStreamOrder::default(),
            watcher_diff: Arc::new(Mutex::new(None)),
            _watcher: None,
            watched_dir: None,
            scan_one_fn: None,
            content_ext: None,
        }
    }
}

impl ContentListState {
    pub fn apply_manifest(
        &mut self,
        manifest: &crate::instance::ContentManifest,
        minecraft_dir: &Path,
        kind: crate::instance::ContentKind,
    ) {
        let mut changed = false;
        let mut invalidated_icons = Vec::new();
        for entry in &mut self.entries {
            let Ok(relative_path) = entry.path.strip_prefix(minecraft_dir) else {
                continue;
            };
            let Some(record) = manifest
                .record(relative_path)
                .filter(|record| record.kind == kind)
            else {
                if entry.provider_project.take().is_some() {
                    if entry.provider_icon {
                        entry.icon_bytes = None;
                        entry.icon_lines = Some(crate::instance::content::mods::fallback_icon());
                        entry.provider_icon = false;
                        invalidated_icons.push(entry.file_stem.clone());
                    }
                    changed = true;
                }
                continue;
            };
            let project = match &record.resolution {
                crate::instance::Resolution::Resolved { project } => Some(project.clone()),
                _ => None,
            };
            if entry.provider_project != project {
                if entry.provider_icon {
                    entry.icon_bytes = None;
                    entry.icon_lines = Some(crate::instance::content::mods::fallback_icon());
                    entry.provider_icon = false;
                    invalidated_icons.push(entry.file_stem.clone());
                }
                entry.provider_project = project.clone();
                changed = true;
            }
        }
        if changed {
            for stem in invalidated_icons {
                self.image_protocols.remove(&stem);
                self.requested_images.remove(&stem);
                self.pending_entry_images.remove(&stem);
            }
            crate::tui::request_redraw();
        }
    }

    pub fn enable_provider_icons(
        &mut self,
        meta_dir: std::path::PathBuf,
        client: crate::net::HttpClient,
    ) {
        self.provider_icon_meta_dir = Some(meta_dir);
        self.provider_icon_client = Some(client);
    }

    pub fn drain_provider_icons(&mut self) -> bool {
        let pending = match self.pending_provider_icons.lock() {
            Ok(mut pending) => pending.drain(..).collect::<Vec<_>>(),
            Err(_) => return false,
        };
        let mut changed = false;
        for icon in pending {
            for entry in &mut self.entries {
                let matches_project = entry.provider_project.as_ref().is_some_and(|project| {
                    project.provider == icon.provider && project.project_id == icon.project_id
                });
                if matches_project && entry.icon_bytes.is_none() {
                    entry.icon_bytes = Some(icon.bytes.clone());
                    entry.provider_icon = true;
                    changed = true;
                }
            }
        }
        if changed {
            self.images_dirty = true;
            crate::tui::request_redraw();
        }
        changed
    }

    fn request_visible_provider_icons(&mut self, filtered: &[usize], viewport_height: u16) {
        let Some(meta_dir) = self.provider_icon_meta_dir.clone() else {
            return;
        };
        let Some(client) = self.provider_icon_client.clone() else {
            return;
        };
        let projects = self.visible_provider_projects(filtered, viewport_height);
        for project in projects {
            let key = (project.provider.clone(), project.project_id.clone());
            if !self.requested_provider_icons.insert(key) {
                continue;
            }
            let pending = self.pending_provider_icons.clone();
            let slots = PROVIDER_ICON_SLOTS.clone();
            let meta_dir = meta_dir.clone();
            let client = client.clone();
            tokio::spawn(async move {
                let Ok(_permit) = slots.acquire_owned().await else {
                    return;
                };
                match load_provider_icon(&client, &meta_dir, &project.provider, &project.project_id)
                    .await
                {
                    Ok(bytes) if !bytes.is_empty() => {
                        if let Ok(mut pending) = pending.lock() {
                            pending.push(PendingProviderIcon {
                                provider: project.provider,
                                project_id: project.project_id,
                                bytes,
                            });
                            crate::tui::request_redraw();
                        }
                    }
                    Ok(_) => {}
                    Err(error) => tracing::debug!(
                        "Could not load provider icon for {} project {}: {}",
                        project.provider,
                        project.project_id,
                        error
                    ),
                }
            });
        }
    }

    fn visible_provider_projects(
        &self,
        filtered: &[usize],
        viewport_height: u16,
    ) -> Vec<crate::instance::ProviderProject> {
        let mut remaining = viewport_height;
        let first = self.list_state.scroll_offset_index();
        let truncation = self.list_state.scroll_truncation();
        let mut projects = Vec::new();

        for (visible_index, &entry_index) in filtered.iter().enumerate().skip(first) {
            let Some(entry) = self.entries.get(entry_index) else {
                continue;
            };
            let height = self.entry_height(entry);
            let visible_height = if visible_index == first {
                height.saturating_sub(truncation)
            } else {
                height
            };
            if visible_height == 0 {
                continue;
            }
            if entry.icon_bytes.is_none()
                && let Some(project) = entry.provider_project.clone()
                && project.provider == "modrinth"
            {
                projects.push(project);
            }
            remaining = remaining.saturating_sub(visible_height);
            if remaining == 0 {
                break;
            }
        }

        projects.sort_by(|left, right| {
            (&left.provider, &left.project_id).cmp(&(&right.provider, &right.project_id))
        });
        projects.dedup_by(|left, right| {
            left.provider == right.provider && left.project_id == right.project_id
        });
        projects
    }

    fn entry_height(&self, entry: &ContentEntry) -> u16 {
        let icon_rows = entry.icon_lines.as_ref().map_or(0, Vec::len);
        let metadata = self.display_metadata.get(&entry.file_stem);
        let has_second_line = metadata.is_some_and(|metadata| metadata.has_description)
            || entry.footer_label.is_some();
        icon_rows.max(if has_second_line { 2 } else { 1 }) as u16
    }

    pub fn start_stream(&mut self, source: impl Into<String>) -> ContentStream {
        self.start_stream_with_order(source, ContentStreamOrder::Sorted)
    }

    pub fn start_source_stream(&mut self, source: impl Into<String>) -> ContentStream {
        self.start_stream_with_order(source, ContentStreamOrder::Source)
    }

    pub fn refresh_source_stream(&mut self, source: impl Into<String>) -> ContentStream {
        let (sender, receiver) = mpsc::channel();
        self.stream_rx = Some(receiver);
        self.stream_order = ContentStreamOrder::Source;
        self.loaded_for = Some(source.into());
        ContentStream { sender }
    }

    fn start_stream_with_order(
        &mut self,
        source: impl Into<String>,
        order: ContentStreamOrder,
    ) -> ContentStream {
        self.images_dirty = true;
        self.image_protocols.clear();
        self.requested_images.clear();
        self.pending_entry_images.clear();
        self.requested_provider_icons.clear();
        self.entries.clear();
        self.display_metadata.clear();
        self.list_state = TuiListState::default();
        self.loading = true;
        self.loaded_for = Some(source.into());
        self.update_scrollbar();
        let (sender, receiver) = mpsc::channel();
        self.stream_rx = Some(receiver);
        self.stream_order = order;
        ContentStream { sender }
    }

    pub fn request_image_loads(&mut self, picker: &ratatui_image::picker::Picker) {
        if !self.images_dirty {
            return;
        }
        self.images_dirty = false;

        let use_image_protocol =
            picker.protocol_type() != ratatui_image::picker::ProtocolType::Halfblocks;
        let use_quadrants = crate::config::SETTINGS.ui.image_protocol
            == crate::config::settings::ImageProtocol::Quadrants;
        if !use_image_protocol {
            self.image_protocols.clear();
        }

        let valid_stems: HashSet<&str> = self
            .entries
            .iter()
            .map(|entry| entry.file_stem.as_str())
            .collect();
        self.image_protocols
            .retain(|stem, _| valid_stems.contains(stem.as_str()));
        self.requested_images
            .retain(|stem| valid_stems.contains(stem.as_str()));

        let font_size = picker.font_size();
        for entry in &self.entries {
            if entry.icon_bytes.is_none() || !self.requested_images.insert(entry.file_stem.clone())
            {
                continue;
            }
            let file_stem = entry.file_stem.clone();
            let path = entry.path.clone();
            let bytes = entry.icon_bytes.clone().unwrap_or_default();
            let rows = entry.icon_lines.as_ref().map_or(3, Vec::len) as u32;
            let columns = square_icon_columns(rows as u16, font_size);
            let pending = self.pending_images.clone();

            tokio::spawn(async move {
                let result = tokio::task::spawn_blocking(move || {
                    let Some(image) = image::load_from_memory(&bytes).ok() else {
                        return PendingContentImage {
                            file_stem,
                            path,
                            icon_lines: crate::instance::content::mods::fallback_icon(),
                            image: None,
                        };
                    };
                    let icon_lines = if use_quadrants {
                        crate::instance::content::mods::make_icon_quadrants_from_image(
                            &image,
                            columns,
                            rows as u16,
                        )
                    } else {
                        crate::instance::content::mods::make_icon_pixels_from_image(
                            &image,
                            columns,
                            rows as u16,
                        )
                    };
                    let side = rows * u32::from(font_size.1.max(1));
                    let image = use_image_protocol.then(|| {
                        image.resize_exact(side, side, image::imageops::FilterType::Lanczos3)
                    });
                    PendingContentImage {
                        file_stem,
                        path,
                        icon_lines,
                        image,
                    }
                })
                .await
                .ok();

                if let Some(result) = result
                    && let Ok(mut pending) = pending.lock()
                {
                    pending.push(result);
                    crate::tui::request_redraw();
                }
            });
        }
    }

    pub fn drain_image_loads(&mut self, picker: &ratatui_image::picker::Picker) {
        let images = match self.pending_images.lock() {
            Ok(mut pending) => std::mem::take(&mut *pending),
            Err(_) => return,
        };

        for result in images {
            if let Some(entry) = self
                .entries
                .iter_mut()
                .find(|entry| entry.file_stem == result.file_stem && entry.path == result.path)
            {
                self.pending_entry_images.remove(&result.file_stem);
                entry.icon_lines = Some(result.icon_lines);
                if let Some(image) = result.image {
                    self.image_protocols
                        .insert(result.file_stem, picker.new_resize_protocol(image));
                }
            }
        }
    }

    // drain streaming entries from the initial load. each entry arrives
    // individually and is inserted in sorted position for a smooth fill-in
    pub fn drain_pending(&mut self) -> bool {
        let Some(rx) = &self.stream_rx else {
            return false;
        };

        let mut received = false;
        let mut received_count = 0usize;
        let mut finished = false;
        let mut restore_selected = None;
        loop {
            match rx.try_recv() {
                Ok(ContentStreamUpdate::Entry(entry)) => {
                    received = true;
                    self.images_dirty = true;
                    received_count += 1;
                    if entry.icon_bytes.is_some() || entry.provider_icon {
                        self.pending_entry_images.insert(entry.file_stem.clone());
                    }
                    self.display_metadata
                        .insert(entry.file_stem.clone(), display_metadata(&entry));
                    match self.stream_order {
                        ContentStreamOrder::Sorted => {
                            let pos = self
                                .entries
                                .binary_search_by(|e| {
                                    e.name.to_lowercase().cmp(&entry.name.to_lowercase())
                                })
                                .unwrap_or_else(|i| i);
                            self.entries.insert(pos, entry);
                        }
                        ContentStreamOrder::Source => self.entries.push(entry),
                    }
                }
                Ok(ContentStreamUpdate::Upsert(mut entry)) => {
                    received = true;
                    self.images_dirty = true;
                    received_count += 1;
                    let stem = entry.file_stem.clone();
                    if let Some(existing) = self
                        .entries
                        .iter_mut()
                        .find(|existing| existing.file_stem == stem)
                    {
                        let same_source = existing.path == entry.path
                            && existing.provider_project == entry.provider_project;
                        if entry.icon_bytes.is_none() && same_source {
                            entry.icon_bytes = existing.icon_bytes.take();
                            entry.icon_lines = existing.icon_lines.take();
                            entry.provider_icon = existing.provider_icon;
                        } else if entry.icon_bytes != existing.icon_bytes {
                            self.image_protocols.remove(&entry.file_stem);
                            self.requested_images.remove(&entry.file_stem);
                        }
                        *existing = entry;
                    } else {
                        self.entries.push(entry);
                    }
                    if let Some(entry) = self.entries.iter().find(|entry| entry.file_stem == stem) {
                        self.display_metadata
                            .insert(entry.file_stem.clone(), display_metadata(entry));
                        if (entry.icon_bytes.is_some() || entry.provider_icon)
                            && !self.image_protocols.contains_key(&entry.file_stem)
                        {
                            self.pending_entry_images.insert(stem);
                        } else {
                            self.pending_entry_images.remove(&stem);
                        }
                    }
                }
                Ok(ContentStreamUpdate::Retain(file_stems)) => {
                    received = true;
                    let selected_stem = self.selected_file_stem();
                    self.entries
                        .retain(|entry| file_stems.contains(&entry.file_stem));
                    self.display_metadata
                        .retain(|stem, _| file_stems.contains(stem));
                    self.image_protocols
                        .retain(|stem, _| file_stems.contains(stem));
                    self.requested_images
                        .retain(|stem| file_stems.contains(stem));
                    self.pending_entry_images
                        .retain(|stem| file_stems.contains(stem));
                    restore_selected = Some(selected_stem);
                    self.images_dirty = true;
                }
                Ok(ContentStreamUpdate::Icon {
                    file_stem,
                    path,
                    bytes,
                }) => {
                    received = true;
                    if let Some(entry) = self
                        .entries
                        .iter_mut()
                        .find(|entry| entry.file_stem == file_stem && entry.path == path)
                    {
                        entry.icon_bytes = Some(bytes);
                        entry.provider_icon = true;
                        self.pending_entry_images.insert(file_stem.clone());
                        self.requested_images.remove(&file_stem);
                        self.images_dirty = true;
                    }
                }
                Ok(ContentStreamUpdate::IconUnavailable { file_stem, path }) => {
                    received = true;
                    if let Some(entry) = self
                        .entries
                        .iter_mut()
                        .find(|entry| entry.file_stem == file_stem && entry.path == path)
                    {
                        entry.provider_icon = false;
                        self.pending_entry_images.remove(&file_stem);
                        self.images_dirty = true;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.stream_rx = None;
                    finished = true;
                    break;
                }
            }
        }

        if let Some(selected_stem) = restore_selected {
            self.restore_selected_file_stem(selected_stem.as_deref());
        }

        if received || finished {
            self.loading = false;
            if received_count > 0 {
                tracing::trace!(
                    "Drained {} streamed content entries for {}",
                    received_count,
                    self.loaded_for.as_deref().unwrap_or("<unknown>")
                );
            }
            if finished {
                tracing::debug!(
                    "Finished content scan for {} with {} entries",
                    self.loaded_for.as_deref().unwrap_or("<unknown>"),
                    self.entries.len()
                );
            }
            if self.list_state.selected.is_none() && !self.entries.is_empty() {
                self.list_state.selected = Some(0);
            }
            self.update_scrollbar();
        }
        received || finished
    }

    // pick up the precomputed diff from the notify watcher callback.
    // skip while streaming is in progress to avoid duplicate entries.
    pub fn drain_watcher(&mut self) -> bool {
        if self.stream_rx.is_some() {
            return false;
        }

        let diff = match self.watcher_diff.lock() {
            Ok(mut slot) => slot.take(),
            _ => None,
        };

        let Some(diff) = diff else {
            return false;
        };
        self.images_dirty = true;

        // apply toggles (enabled/path changes)
        tracing::debug!(
            "Applying content watcher diff for {}: toggled={} removed={} added={}",
            self.loaded_for.as_deref().unwrap_or("<unknown>"),
            diff.toggled.len(),
            diff.removed.len(),
            diff.added.len()
        );
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
            for stem in &diff.removed {
                self.display_metadata.remove(stem);
            }
        }

        // insert new entries in sorted position
        for entry in diff.added {
            if entry.icon_bytes.is_some() {
                self.pending_entry_images.insert(entry.file_stem.clone());
            }
            self.display_metadata
                .insert(entry.file_stem.clone(), display_metadata(&entry));
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
                self.list_state.selected = Some(sel.min(self.entries.len().saturating_sub(1)));
            }
        }

        self.update_scrollbar();
        true
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

        let running = Arc::new(AtomicBool::new(false));
        let running_cb = running.clone();
        let pending_paths = Arc::new(Mutex::new(HashSet::<std::path::PathBuf>::new()));
        let pending_paths_cb = pending_paths.clone();
        let needs_rescan = Arc::new(AtomicBool::new(false));
        let needs_rescan_cb = needs_rescan.clone();

        // initialize known stems from the current directory state so existing
        // files are not treated as "new" on the first notify event
        let known_stems = Arc::new(Mutex::new(read_dir_stems(&dir, ext)));

        let watch_dir = dir.clone();
        let watcher = notify::recommended_watcher(move |res: Result<notify::Event, _>| {
            let event_paths = match res {
                Ok(event) => match watcher_event_handling(&event.kind) {
                    WatcherEventHandling::Ignore => return,
                    WatcherEventHandling::Paths => event.paths,
                    WatcherEventHandling::Rescan => {
                        needs_rescan_cb.store(true, Ordering::Relaxed);
                        event.paths
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "Content watcher event error for {}: {}",
                        watch_dir.display(),
                        e
                    );
                    needs_rescan_cb.store(true, Ordering::Relaxed);
                    Vec::new()
                }
            };
            if let Ok(mut pending) = pending_paths_cb.lock() {
                pending.extend(event_paths);
            }

            if running_cb.swap(true, Ordering::Relaxed) {
                return;
            }

            let dir = watch_dir.clone();
            let diff_slot = watcher_diff.clone();
            let running = running_cb.clone();
            let known = known_stems.clone();
            let pending_paths = pending_paths_cb.clone();
            let needs_rescan = needs_rescan_cb.clone();

            std::thread::spawn(move || {
                // always clear `running` even if we panic
                struct ResetOnDrop(Arc<AtomicBool>);
                impl Drop for ResetOnDrop {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::Relaxed);
                    }
                }
                let _guard = ResetOnDrop(running.clone());

                loop {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let paths = pending_paths
                        .lock()
                        .ok()
                        .map(|mut pending| pending.drain().collect::<Vec<_>>())
                        .unwrap_or_default();
                    let full_rescan = needs_rescan.swap(false, Ordering::Relaxed);
                    let result = if full_rescan {
                        diff_directory(&dir, ext, scan_one, &known)
                    } else {
                        diff_event_paths(&dir, &paths, ext, scan_one, &known)
                    };

                    if let Some(diff) = result
                        && let Ok(mut slot) = diff_slot.lock()
                    {
                        if let Some(pending) = slot.as_mut() {
                            merge_watcher_diff(pending, diff);
                        } else {
                            *slot = Some(diff);
                        }
                        crate::tui::request_redraw();
                    }

                    let no_pending = pending_paths.lock().is_ok_and(|pending| pending.is_empty());
                    if no_pending && !needs_rescan.load(Ordering::Relaxed) {
                        // Release ownership before the final check. An event
                        // racing with this boundary either starts a new worker
                        // or is observed here and kept by this worker.
                        running.store(false, Ordering::Release);
                        let still_empty =
                            pending_paths.lock().is_ok_and(|pending| pending.is_empty());
                        if still_empty && !needs_rescan.load(Ordering::Acquire) {
                            break;
                        }
                        if running
                            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                            .is_err()
                        {
                            break;
                        }
                    }
                }
            });
        });

        match watcher {
            Ok(mut w) => {
                if let Err(e) = w.watch(&dir, RecursiveMode::NonRecursive) {
                    tracing::warn!("Failed to watch {}: {e}", dir.display());
                } else {
                    tracing::debug!("Watching content directory {}", dir.display());
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
            .filter(|(_, entry)| {
                !self.pending_entry_images.contains(&entry.file_stem)
                    && (!self.filter_search
                        || self.search.matches(&entry.name)
                        || self.search.matches(&entry.description))
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn set_search_filtering(&mut self, enabled: bool) {
        let selected_stem = self.selected_file_stem();
        self.filter_search = enabled;
        self.restore_selected_file_stem(selected_stem.as_deref());
    }

    fn selected_file_stem(&self) -> Option<String> {
        let filtered = self.filtered_indices();
        let entry_index = self
            .list_state
            .selected
            .and_then(|index| filtered.get(index))?;
        self.entries
            .get(*entry_index)
            .map(|entry| entry.file_stem.clone())
    }

    fn restore_selected_file_stem(&mut self, file_stem: Option<&str>) {
        let filtered = self.filtered_indices();
        self.list_state.selected = file_stem
            .and_then(|stem| {
                filtered.iter().position(|index| {
                    self.entries
                        .get(*index)
                        .is_some_and(|entry| entry.file_stem == stem)
                })
            })
            .or_else(|| (!filtered.is_empty()).then_some(0));
        self.update_scrollbar();
    }

    pub fn pending_delete(&self) -> Option<PendingContentDelete> {
        let filtered = self.filtered_indices();
        let real_idx = self.list_state.selected.and_then(|i| filtered.get(i))?;
        let entry = self.entries.get(*real_idx)?;
        Some(PendingContentDelete {
            name: entry.name.clone(),
            path: entry.path.clone(),
        })
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
        self.images_dirty = true;
        self.image_protocols.clear();
        self.requested_images.clear();
        self.pending_entry_images.clear();

        // save current entries to cache
        if let Some(prev) = self.loaded_for.take()
            && !self.entries.is_empty()
        {
            tracing::trace!(
                "Caching {} content entries for {}",
                self.entries.len(),
                prev
            );
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
            self.pending_entry_images.extend(
                self.entries
                    .iter()
                    .filter(|entry| entry.icon_bytes.is_some())
                    .map(|entry| entry.file_stem.clone()),
            );
            self.rebuild_display_metadata();
            self.list_state.selected = cached.selected;
            self.loading = false;
            self.stream_rx = None;
            self.loaded_for = Some(instance_name.to_string());
            self.update_scrollbar();
            tracing::debug!(
                "Restored {} cached content entries for {}",
                self.entries.len(),
                instance_name
            );
            return;
        }

        // no cache, stream entries one by one as each file is scanned
        let stream = self.start_stream(instance_name);

        let dir = content_dir.to_path_buf();
        tracing::debug!(
            "Starting content scan for {} in {}",
            instance_name,
            content_dir.display()
        );

        tokio::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                let read_dir = match std::fs::read_dir(&dir) {
                    Ok(rd) => rd,
                    Err(e) => {
                        tracing::warn!("Failed to read content directory {}: {}", dir.display(), e);
                        return;
                    }
                };
                let disabled_ext = format!("{ext}.disabled");

                for dir_entry in read_dir.flatten() {
                    let path = dir_entry.path();
                    let Some(fname) = path.file_name().and_then(|n| n.to_str()) else {
                        tracing::trace!(
                            "Skipping content path with invalid filename: {}",
                            path.display()
                        );
                        continue;
                    };

                    let (enabled, file_stem) = if let Some(stem) = fname.strip_suffix(&disabled_ext)
                    {
                        (false, stem.to_owned())
                    } else if let Some(stem) = fname.strip_suffix(ext) {
                        (true, stem.to_owned())
                    } else if path.is_dir() {
                        crate::instance::content::parse_enabled_stem_dir(fname)
                    } else {
                        tracing::trace!(
                            "Skipping content path with unsupported extension: {}",
                            path.display()
                        );
                        continue;
                    };

                    let entry = scan_one_fn(&path, &file_stem, enabled);
                    if !stream.send(entry) {
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

    fn rebuild_display_metadata(&mut self) {
        self.display_metadata = self
            .entries
            .iter()
            .map(|entry| (entry.file_stem.clone(), display_metadata(entry)))
            .collect();
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
                tracing::error!(
                    "Failed to toggle '{}' from {} to {}: {}",
                    entry.file_stem,
                    entry.path.display(),
                    new_path.display(),
                    e
                );
            }
        }
    }

    pub fn remove_path(&mut self, path: &Path) {
        let file_stem = self
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .map(|entry| entry.file_stem.clone());
        self.entries.retain(|entry| entry.path != path);
        if let Some(file_stem) = file_stem {
            self.image_protocols.remove(&file_stem);
            self.requested_images.remove(&file_stem);
            self.display_metadata.remove(&file_stem);
        }
        self.images_dirty = true;
        if let Some(sel) = self.list_state.selected {
            let visible_count = self.filtered_indices().len();
            if visible_count == 0 {
                self.list_state.selected = None;
            } else {
                self.list_state.selected = Some(sel.min(visible_count.saturating_sub(1)));
            }
        }
        self.update_scrollbar();
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

async fn load_provider_icon(
    client: &crate::net::HttpClient,
    meta_dir: &Path,
    provider_id: &str,
    project_id: &str,
) -> Result<Vec<u8>, crate::net::NetError> {
    if provider_id != "modrinth" {
        return Err(crate::net::NetError::Parse(format!(
            "Content provider '{provider_id}' does not support lazy icons"
        )));
    }
    let metadata = crate::storage::MetadataPaths::new(meta_dir);
    let icon_path = metadata
        .provider_icons(provider_id)
        .join(format!("{project_id}.img"));
    if let Ok(bytes) = tokio::fs::read(&icon_path).await
        && !bytes.is_empty()
        && image::load_from_memory(&bytes).is_ok()
    {
        return Ok(bytes);
    }

    let registry = crate::content_provider::ProviderRegistry::modrinth(client.clone());
    let provider = registry.preferred(provider_id).ok_or_else(|| {
        crate::net::NetError::Parse(format!("Content provider '{provider_id}' is unavailable"))
    })?;
    let project_path = metadata
        .provider_projects(provider_id)
        .join(format!("{project_id}.json"));
    let cached_project = tokio::fs::read(&project_path)
        .await
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    let project = match cached_project {
        Some(project) => project,
        None => {
            let project = provider.project(project_id).await?;
            crate::storage::write_atomic(
                &project_path,
                &serde_json::to_vec_pretty(&project)
                    .map_err(|error| crate::net::NetError::Parse(error.to_string()))?,
            )?;
            project
        }
    };
    let Some(url) = project.icon_url.as_deref() else {
        return Ok(Vec::new());
    };
    let bytes = provider.icon(url).await?;
    if bytes.is_empty() || image::load_from_memory(&bytes).is_err() {
        return Err(crate::net::NetError::Parse(format!(
            "Provider returned an invalid icon for project '{project_id}'"
        )));
    }
    crate::storage::write_atomic(&icon_path, &bytes)?;
    Ok(bytes)
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
                && let Err(e) = open::that_detached(dir)
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
                && let Err(e) = open::that_detached(dir)
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
    picker: &ratatui_image::picker::Picker,
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
        let text = if state.pending_entry_images.is_empty() {
            empty_text
        } else {
            loading_text
        };
        frame.render_widget(
            Paragraph::new(text).style(Style::default().fg(theme.text_dim())),
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
    state.request_visible_provider_icons(&filtered, area.height);

    let use_image_protocol =
        picker.protocol_type() != ratatui_image::picker::ProtocolType::Halfblocks;
    let entries = &state.entries;
    let display_metadata = &state.display_metadata;
    let filtered_rows = &filtered;
    let search = &state.search;
    let ready_image_stems: HashSet<String> = state.image_protocols.keys().cloned().collect();

    let builder = ListBuilder::new(move |context| {
        let theme = THEME.as_ref();
        let entry = &entries[filtered_rows[context.index]];
        let name = &entry.name;
        let metadata = display_metadata.get(&entry.file_stem);
        let enabled = entry.enabled;
        let icon_pixels = &entry.icon_lines;
        let title_suffix = entry.title_suffix.as_deref();
        let footer_label = entry.footer_label.as_deref();
        // Keep rendering the terminal fallback until the asynchronous image
        // decoder has produced a protocol. Invalid and unsupported images
        // therefore remain visible as a question mark instead of blank space.
        let has_image = ready_image_stems.contains(&entry.file_stem);
        let protocol_columns = protocol_icon_columns(entry, picker) as usize;
        let show_selected = is_focused && context.is_selected;
        let use_mc_colors = enabled;

        let stripe_bg = if context.index % 2 == 0 {
            theme.background()
        } else {
            theme.stripe()
        };

        let (name_style, description_style, background) = match (enabled, show_selected) {
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
        let title_suffix_style = Style::default()
            .fg(theme.background())
            .bg(theme.success())
            .add_modifier(Modifier::BOLD);
        let footer_label_style = Style::default().fg(theme.text());

        let has_icon = icon_pixels.is_some();
        let stripped_desc = metadata.map_or("", |metadata| metadata.description.as_str());
        let has_description = metadata.is_some_and(|metadata| metadata.has_description);
        let rendered_icon_columns = if use_image_protocol && has_image {
            protocol_columns
        } else {
            icon_pixels
                .as_ref()
                .and_then(|rows| rows.first())
                .map_or(0, Vec::len)
        };
        let description_width = available_description_width(
            usize::from(context.cross_axis_size),
            rendered_icon_columns,
            has_icon,
        );
        let visible_description = ellipsize(
            stripped_desc,
            description_text_width(description_width, footer_label, has_description),
        );
        let compact = !has_icon && !has_description && footer_label.is_none();

        let selector = if show_selected {
            Span::styled("\u{258c}", Style::default().fg(theme.accent()))
        } else {
            Span::raw(" ")
        };

        if compact {
            let mut line = Vec::new();
            line.push(selector.clone());
            line.extend(searchable_spans(search, name, name_style, use_mc_colors));
            line.extend(title_suffix_spans(
                title_suffix,
                description_style,
                title_suffix_style,
            ));

            let item = Text::from(vec![Line::from(line)]).style(Style::default().bg(background));
            (item, 1)
        } else if has_icon {
            let icon_row_count = icon_pixels.as_ref().map(|r| r.len()).unwrap_or(0);
            let text_rows = if has_description || footer_label.is_some() {
                2
            } else {
                1
            };
            let height = icon_row_count.max(text_rows) as u16;

            let pad = if show_selected {
                Span::styled("\u{258c}", Style::default().fg(theme.accent()))
            } else {
                Span::raw(" ")
            };

            let mut line_0 = vec![selector.clone()];
            line_0.extend(icon_spans(
                icon_pixels.as_ref(),
                0,
                use_image_protocol && has_image,
                protocol_columns,
            ));
            line_0.push(Span::raw(" "));
            line_0.extend(searchable_spans(search, name, name_style, use_mc_colors));
            line_0.extend(title_suffix_spans(
                title_suffix,
                description_style,
                title_suffix_style,
            ));

            let mut lines = vec![Line::from(line_0)];

            if has_description || footer_label.is_some() {
                let mut row = vec![pad.clone()];
                row.extend(icon_spans(
                    icon_pixels.as_ref(),
                    1,
                    use_image_protocol && has_image,
                    protocol_columns,
                ));
                row.push(Span::raw(" "));
                if has_description {
                    row.extend(search.highlight_spans(&visible_description, description_style));
                }
                if let Some(footer_label) = footer_label {
                    row.extend(right_aligned_footer_spans(
                        description_width,
                        &visible_description,
                        has_description,
                        footer_label,
                        footer_label_style,
                    ));
                }
                lines.push(Line::from(row));
            }

            for r in text_rows..icon_row_count {
                let mut row = vec![pad.clone()];
                row.extend(icon_spans(
                    icon_pixels.as_ref(),
                    r,
                    use_image_protocol && has_image,
                    protocol_columns,
                ));
                lines.push(Line::from(row));
            }

            let item = Text::from(lines).style(Style::default().bg(background));
            (item, height)
        } else {
            let mut line_0 = Vec::new();
            line_0.push(selector.clone());
            line_0.extend(searchable_spans(search, name, name_style, use_mc_colors));
            line_0.extend(title_suffix_spans(
                title_suffix,
                description_style,
                title_suffix_style,
            ));

            let mut lines = vec![Line::from(line_0)];

            if has_description || footer_label.is_some() {
                let pad = if show_selected {
                    Span::styled("\u{258c}", Style::default().fg(theme.accent()))
                } else {
                    Span::raw(" ")
                };
                let mut description = vec![pad];
                if has_description {
                    description
                        .extend(search.highlight_spans(&visible_description, description_style));
                }
                if let Some(footer_label) = footer_label {
                    description.extend(right_aligned_footer_spans(
                        description_width,
                        &visible_description,
                        has_description,
                        footer_label,
                        footer_label_style,
                    ));
                }
                lines.push(Line::from(description));
            }

            let height = lines.len() as u16;
            let item = Text::from(lines).style(Style::default().bg(background));
            (item, height)
        }
    });

    let list = ListView::new(builder, count);
    frame.render_stateful_widget(list, area, &mut state.list_state);

    if picker.protocol_type() != ratatui_image::picker::ProtocolType::Halfblocks {
        render_image_icons(frame, area, state, &filtered, picker);
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

fn render_image_icons(
    frame: &mut Frame,
    area: Rect,
    state: &mut ContentListState,
    filtered: &[usize],
    picker: &ratatui_image::picker::Picker,
) {
    let truncation = state.list_state.scroll_truncation();
    let mut y = area.y;
    let first = state.list_state.scroll_offset_index();

    for (visible_index, &entry_index) in filtered.iter().enumerate().skip(first) {
        let Some(entry) = state.entries.get(entry_index) else {
            continue;
        };
        let icon_rows = entry.icon_lines.as_ref().map_or(0, Vec::len);
        let has_description = state
            .display_metadata
            .get(&entry.file_stem)
            .is_some_and(|metadata| metadata.has_description);
        let height = icon_rows.max(if has_description { 2 } else { 1 }) as u16;

        if y >= area.y + area.height {
            break;
        }
        let top_crop = if visible_index == first {
            truncation.min(icon_rows as u16)
        } else {
            0
        };
        let visible_icon_rows = (icon_rows as u16)
            .saturating_sub(top_crop)
            .min(area.y + area.height - y);
        if visible_icon_rows > 0
            && entry.icon_bytes.is_some()
            && let Some(protocol) = state.image_protocols.get_mut(&entry.file_stem)
        {
            let icon_area = Rect {
                x: area.x + 1,
                y,
                width: protocol_icon_columns(entry, picker).min(area.width.saturating_sub(1)),
                height: visible_icon_rows,
            };
            if icon_area.height > 0 && icon_area.width > 0 {
                let clipped = top_crop > 0 || visible_icon_rows < icon_rows as u16;
                let resize = if clipped {
                    Resize::Crop(Some(CropOptions {
                        clip_top: top_crop > 0,
                        clip_left: false,
                    }))
                } else {
                    Resize::Scale(None)
                };
                let widget: StatefulImage<StatefulProtocol> =
                    StatefulImage::default().resize(resize);
                frame.render_stateful_widget(widget, icon_area, protocol);
            }
        }
        let visible_height = if visible_index == first {
            height.saturating_sub(truncation)
        } else {
            height
        };
        y = y.saturating_add(visible_height);
        if visible_index + 1 >= filtered.len() {
            break;
        }
    }
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

fn searchable_spans(
    search: &crate::tui::widgets::search::SearchState,
    text: &str,
    base_style: Style,
    use_mc_colors: bool,
) -> Vec<Span<'static>> {
    if !search.is_empty() {
        search.highlight_spans(&strip_mc_codes(text), base_style)
    } else if use_mc_colors {
        parse_mc_text(text, base_style)
    } else {
        vec![Span::styled(strip_mc_codes(text), base_style)]
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

fn display_metadata(entry: &ContentEntry) -> DisplayMetadata {
    let description = strip_mc_codes(&entry.description);
    let description = description.lines().next().unwrap_or("").trim().to_string();
    DisplayMetadata {
        has_description: !description.is_empty(),
        description,
    }
}

// renders one row of a mod icon using half-block characters (U+2584).
// each cell packs two vertical pixels via fg/bg colors, giving
// double the vertical resolution out of the terminal
fn icon_spans(
    icon_pixels: Option<&Vec<Vec<IconCell>>>,
    row: usize,
    use_image_protocol: bool,
    protocol_columns: usize,
) -> Vec<Span<'static>> {
    if use_image_protocol {
        return vec![Span::raw(" ".repeat(protocol_columns))];
    }
    match icon_pixels.and_then(|rows| rows.get(row)) {
        Some(cols) => cols
            .iter()
            .map(|cell| {
                Span::styled(
                    cell.symbol.to_string(),
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

fn title_suffix_spans(
    suffix: Option<&str>,
    spacing_style: Style,
    label_style: Style,
) -> Vec<Span<'static>> {
    suffix.map_or_else(Vec::new, |suffix| {
        vec![
            Span::styled("  ", spacing_style),
            Span::styled(format!(" {suffix} "), label_style),
        ]
    })
}

fn available_description_width(
    row_width: usize,
    rendered_icon_columns: usize,
    has_icon: bool,
) -> usize {
    let selector_and_scrollbar = 2;
    let icon_and_gap = if has_icon {
        rendered_icon_columns + 1
    } else {
        0
    };
    row_width.saturating_sub(selector_and_scrollbar + icon_and_gap)
}

fn description_text_width(
    available_width: usize,
    footer_label: Option<&str>,
    has_description: bool,
) -> usize {
    let footer_width = footer_label.map_or(0, |footer_label| {
        Span::raw(footer_label).width() + usize::from(has_description)
    });
    available_width.saturating_sub(footer_width)
}

fn right_aligned_footer_spans(
    available_width: usize,
    description: &str,
    has_description: bool,
    footer_label: &str,
    footer_style: Style,
) -> Vec<Span<'static>> {
    let description_width = if has_description {
        Span::raw(description).width()
    } else {
        0
    };
    let footer_width = Span::raw(footer_label).width();
    let padding = available_width.saturating_sub(description_width + footer_width);
    vec![
        Span::raw(" ".repeat(padding)),
        Span::styled(footer_label.to_owned(), footer_style),
    ]
}

fn ellipsize(text: &str, max_width: usize) -> String {
    if Span::raw(text).width() <= max_width {
        return text.to_owned();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let content_width = max_width - 3;
    let mut visible = String::new();
    for character in text.chars() {
        visible.push(character);
        if Span::raw(visible.as_str()).width() > content_width {
            visible.pop();
            break;
        }
    }
    visible = visible.trim_end().to_owned();
    visible.push_str("...");
    visible
}

fn protocol_icon_columns(
    entry: &crate::instance::content::mods::ContentEntry,
    picker: &ratatui_image::picker::Picker,
) -> u16 {
    let rows = entry.icon_lines.as_ref().map_or(3, Vec::len) as u16;
    square_icon_columns(rows, picker.font_size())
}

fn square_icon_columns(rows: u16, font_size: (u16, u16)) -> u16 {
    let width = u32::from(font_size.0.max(1));
    let height = u32::from(font_size.1.max(1));
    ((u32::from(rows) * height + width / 2) / width).max(1) as u16
}

#[derive(Debug, PartialEq, Eq)]
enum WatcherEventHandling {
    Ignore,
    Paths,
    Rescan,
}

fn watcher_event_handling(kind: &notify::EventKind) -> WatcherEventHandling {
    match kind {
        notify::EventKind::Access(_) => WatcherEventHandling::Ignore,
        notify::EventKind::Create(_)
        | notify::EventKind::Modify(_)
        | notify::EventKind::Remove(_) => WatcherEventHandling::Paths,
        notify::EventKind::Any | notify::EventKind::Other => WatcherEventHandling::Rescan,
    }
}

// reads a content directory and builds a stem -> (path, enabled) map.
// used both by watch_dir to initialize known state and by the watcher
// thread to detect changes. when ext is empty (worlds), only directories
// are included.
fn diff_directory(
    dir: &std::path::Path,
    ext: &str,
    scan_one: Option<ScanOneFn>,
    known: &Arc<Mutex<HashMap<String, (std::path::PathBuf, bool)>>>,
) -> Option<WatcherDiff> {
    let on_disk = read_dir_stems(dir, ext);
    let mut known_map = known.lock().ok()?;
    let mut toggled = Vec::new();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for (stem, (old_path, old_enabled)) in known_map.iter() {
        if let Some((disk_path, disk_enabled)) = on_disk.get(stem) {
            if disk_enabled != old_enabled || disk_path != old_path {
                toggled.push((stem.clone(), *disk_enabled, disk_path.clone()));
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
    watcher_diff(toggled, removed, added)
}

fn diff_event_paths(
    dir: &std::path::Path,
    paths: &[std::path::PathBuf],
    ext: &str,
    scan_one: Option<ScanOneFn>,
    known: &Arc<Mutex<HashMap<String, (std::path::PathBuf, bool)>>>,
) -> Option<WatcherDiff> {
    let mut known_map = known.lock().ok()?;
    let mut toggled = Vec::new();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    for path in paths {
        if path.parent() != Some(dir) {
            continue;
        }
        if path.exists() {
            let Some((stem, enabled)) = watched_stem(path, ext) else {
                continue;
            };
            match known_map.get(&stem) {
                Some((known_path, known_enabled))
                    if known_path == path && *known_enabled == enabled =>
                {
                    // A modify event still needs a rescan so changed archive
                    // metadata and icons become visible.
                    if let Some(scan_one) = scan_one {
                        removed.push(stem.clone());
                        added.push(scan_one(path, &stem, enabled));
                    }
                }
                Some(_) => {
                    toggled.push((stem.clone(), enabled, path.clone()));
                }
                None => {
                    if let Some(scan_one) = scan_one {
                        added.push(scan_one(path, &stem, enabled));
                    }
                }
            }
            known_map.insert(stem, (path.clone(), enabled));
        } else {
            let removed_stems = known_map
                .iter()
                .filter(|(_, (known_path, _))| known_path == path)
                .map(|(stem, _)| stem.clone())
                .collect::<Vec<_>>();
            for stem in removed_stems {
                known_map.remove(&stem);
                removed.push(stem);
            }
        }
    }
    watcher_diff(toggled, removed, added)
}

fn watcher_diff(
    toggled: Vec<(String, bool, std::path::PathBuf)>,
    removed: Vec<String>,
    added: Vec<crate::instance::content::mods::ContentEntry>,
) -> Option<WatcherDiff> {
    if toggled.is_empty() && removed.is_empty() && added.is_empty() {
        None
    } else {
        Some(WatcherDiff {
            toggled,
            removed,
            added,
        })
    }
}

fn merge_watcher_diff(pending: &mut WatcherDiff, mut next: WatcherDiff) {
    pending.toggled.append(&mut next.toggled);
    pending.removed.append(&mut next.removed);
    pending.added.append(&mut next.added);

    pending.toggled.reverse();
    pending.toggled.sort_by(|left, right| left.0.cmp(&right.0));
    pending.toggled.dedup_by(|left, right| left.0 == right.0);
    pending.removed.sort();
    pending.removed.dedup();
    pending.added.reverse();
    pending
        .added
        .sort_by(|left, right| left.file_stem.cmp(&right.file_stem));
    pending
        .added
        .dedup_by(|left, right| left.file_stem == right.file_stem);
}

fn watched_stem(path: &std::path::Path, ext: &str) -> Option<(String, bool)> {
    let name = path.file_name()?.to_str()?;
    if path.is_dir() || ext.is_empty() {
        let (enabled, stem) = crate::instance::content::parse_enabled_stem_dir(name);
        return Some((stem, enabled));
    }
    let disabled_ext = format!("{ext}.disabled");
    if let Some(stem) = name.strip_suffix(&disabled_ext) {
        Some((stem.to_owned(), false))
    } else {
        name.strip_suffix(ext).map(|stem| (stem.to_owned(), true))
    }
}

fn read_dir_stems(dir: &std::path::Path, ext: &str) -> HashMap<String, (std::path::PathBuf, bool)> {
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

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, HashSet},
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use crate::instance::content::mods::ContentEntry;
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        style::{Color, Modifier, Style},
        text::{Line, Span, Text},
        widgets::Widget,
    };

    use super::{
        ContentListState, WatcherEventHandling, available_description_width,
        description_text_width, diff_event_paths, ellipsize, load_provider_icon,
        right_aligned_footer_spans, square_icon_columns, title_suffix_spans,
        watcher_event_handling,
    };

    fn entry(name: &str) -> ContentEntry {
        ContentEntry {
            file_stem: name.to_lowercase(),
            name: name.to_owned(),
            source_slug: None,
            installed_path: None,
            provider_project: None,
            title_suffix: None,
            footer_label: None,
            description: String::new(),
            enabled: true,
            icon_bytes: None,
            provider_icon: false,
            path: PathBuf::from(name.to_lowercase()),
            icon_lines: None,
        }
    }

    #[test]
    fn content_watcher_ignores_file_access_events() {
        assert_eq!(
            watcher_event_handling(&notify::EventKind::Access(notify::event::AccessKind::Any)),
            WatcherEventHandling::Ignore
        );
    }

    #[test]
    fn content_watcher_handles_mutations_without_full_rescan() {
        assert_eq!(
            watcher_event_handling(&notify::EventKind::Modify(notify::event::ModifyKind::Any)),
            WatcherEventHandling::Paths
        );
        assert_eq!(
            watcher_event_handling(&notify::EventKind::Any),
            WatcherEventHandling::Rescan
        );
    }

    #[test]
    fn irrelevant_watcher_paths_do_not_emit_an_empty_diff() {
        let temp = tempfile::tempdir().unwrap();
        let known = Arc::new(Mutex::new(HashMap::new()));
        let paths = vec![temp.path().join("notes.txt")];
        assert!(diff_event_paths(temp.path(), &paths, ".jar", None, &known).is_none());
    }

    #[test]
    fn square_columns_follow_terminal_cell_ratio() {
        assert_eq!(square_icon_columns(3, (8, 16)), 6);
        assert_eq!(square_icon_columns(3, (8, 18)), 7);
        assert_eq!(square_icon_columns(6, (8, 18)), 14);
    }

    #[test]
    fn square_columns_handle_missing_cell_size() {
        assert_eq!(square_icon_columns(3, (0, 0)), 3);
    }

    #[test]
    fn title_badge_is_rendered_after_a_small_gap() {
        let label_style = Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let spans = title_suffix_spans(Some("Installed"), Style::default(), label_style);
        let text = spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<Vec<_>>()
            .concat();

        assert_eq!(text, "   Installed ");
        assert_eq!(spans[1].style, label_style);
        assert!(title_suffix_spans(None, Style::default(), label_style).is_empty());
    }

    #[test]
    fn title_suffix_keeps_label_style_after_the_row_background_is_applied() {
        let label_style = Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD);
        let text = Text::from(Line::from(title_suffix_spans(
            Some("downloads"),
            Style::default(),
            label_style,
        )))
        .style(Style::default().bg(Color::Black));
        let area = Rect::new(0, 0, 20, 1);
        let mut buffer = Buffer::empty(area);

        text.render(area, &mut buffer);

        let label_cell = buffer.cell((5, 0)).unwrap();
        assert_eq!(label_cell.fg, Color::Black);
        assert_eq!(label_cell.bg, Color::Cyan);
        assert!(label_cell.modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn descriptions_are_ellipsized_to_the_available_cell_width() {
        assert_eq!(ellipsize("short", 5), "short");
        assert_eq!(ellipsize("a longer description", 10), "a longe...");
        assert_eq!(ellipsize("narrow", 3), "...");
        assert_eq!(ellipsize("narrow", 2), "..");
        assert_eq!(ellipsize("界界界", 5), "界...");
    }

    #[test]
    fn description_width_reserves_the_row_chrome() {
        assert_eq!(available_description_width(100, 6, true), 91);
        assert_eq!(available_description_width(100, 0, false), 98);
        assert_eq!(available_description_width(4, 6, true), 0);
    }

    #[test]
    fn description_width_reserves_the_download_metadata() {
        assert_eq!(description_text_width(40, Some("1.2K downloads"), true), 25);
        assert_eq!(description_text_width(10, Some("1.2K downloads"), true), 0);
        assert_eq!(description_text_width(40, None, true), 40);
    }

    #[test]
    fn footer_metadata_is_right_aligned_without_a_separator() {
        let mut spans = vec![Span::raw("Description")];
        spans.extend(right_aligned_footer_spans(
            30,
            "Description",
            true,
            "1.2K downloads",
            Style::default(),
        ));
        let line = Line::from(spans);

        assert_eq!(line.width(), 30);
        assert_eq!(line.to_string(), "Description     1.2K downloads");
    }

    #[test]
    fn content_stream_inserts_entries_and_icons_incrementally() {
        let mut state = ContentListState::default();
        let stream = state.start_stream("remote");

        assert!(stream.send(entry("Zulu")));
        state.drain_pending();
        assert_eq!(state.entries[0].name, "Zulu");
        assert!(!state.loading);

        assert!(stream.send(entry("Alpha")));
        assert!(stream.send_icon("alpha".to_owned(), PathBuf::from("alpha"), vec![1, 2, 3],));
        state.drain_pending();

        assert_eq!(
            state
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["Alpha", "Zulu"]
        );
        assert_eq!(
            state.entries[0].icon_bytes.as_deref(),
            Some([1, 2, 3].as_slice())
        );
    }

    #[test]
    fn source_stream_preserves_remote_result_order() {
        let mut state = ContentListState::default();
        let stream = state.start_source_stream("remote");
        assert!(stream.send(entry("Zulu")));
        assert!(stream.send(entry("Alpha")));

        state.drain_pending();

        assert_eq!(
            state
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["Zulu", "Alpha"]
        );
    }

    #[test]
    fn source_refresh_reconciles_without_rebuilding_unchanged_entries() {
        let mut state = ContentListState::default();
        let initial = state.start_source_stream("remote");
        let mut alpha = entry("Alpha");
        alpha.icon_bytes = Some(vec![1, 2, 3]);
        assert!(initial.upsert(alpha));
        assert!(initial.upsert(entry("Beta")));
        assert!(initial.upsert(entry("Gamma")));
        state.drain_pending();
        state.list_state.selected = Some(0);

        let refresh = state.refresh_source_stream("remote");
        let mut alpha_update = entry("Alpha");
        alpha_update.description = "Updated".to_owned();
        assert!(refresh.upsert(alpha_update));
        assert!(refresh.upsert(entry("Delta")));
        assert!(refresh.retain(HashSet::from(["alpha".to_owned(), "delta".to_owned(),])));
        state.drain_pending();

        assert_eq!(
            state
                .entries
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            ["Alpha", "Delta"]
        );
        assert_eq!(state.entries[0].description, "Updated");
        assert_eq!(
            state.entries[0].icon_bytes.as_deref(),
            Some([1, 2, 3].as_slice())
        );
        assert_eq!(state.list_state.selected, Some(0));
        assert!(!state.loading);
    }

    #[test]
    fn provider_icons_are_requested_only_for_visible_missing_icons() {
        let mut state = ContentListState::default();
        let mut visible = entry("Visible");
        visible.icon_lines = Some(crate::instance::content::mods::fallback_icon());
        visible.provider_project = Some(crate::instance::ProviderProject {
            provider: "modrinth".to_owned(),
            project_id: "visible-project".to_owned(),
            version_id: "version".to_owned(),
        });
        let mut offscreen = entry("Offscreen");
        offscreen.icon_lines = Some(crate::instance::content::mods::fallback_icon());
        offscreen.provider_project = Some(crate::instance::ProviderProject {
            provider: "modrinth".to_owned(),
            project_id: "offscreen-project".to_owned(),
            version_id: "version".to_owned(),
        });
        state.entries = vec![visible, offscreen];
        state.rebuild_display_metadata();

        let projects = state.visible_provider_projects(&[0, 1], 3);

        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].project_id, "visible-project");
    }

    #[test]
    fn embedded_icons_do_not_request_provider_fallbacks() {
        let mut state = ContentListState::default();
        let mut visible = entry("Visible");
        visible.icon_bytes = Some(vec![1, 2, 3]);
        visible.icon_lines = Some(crate::instance::content::mods::fallback_icon());
        visible.provider_project = Some(crate::instance::ProviderProject {
            provider: "modrinth".to_owned(),
            project_id: "visible-project".to_owned(),
            version_id: "version".to_owned(),
        });
        state.entries = vec![visible];
        state.rebuild_display_metadata();

        assert!(state.visible_provider_projects(&[0], 3).is_empty());
    }

    #[tokio::test]
    async fn streamed_entries_wait_for_their_rendered_icon() {
        let mut state = ContentListState::default();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let mut with_icon = entry("With icon");
        with_icon.icon_bytes = Some(png.into_inner());
        let stream = state.start_stream("local");

        assert!(stream.send(with_icon));
        state.drain_pending();
        assert!(state.filtered_indices().is_empty());

        let picker = ratatui_image::picker::Picker::halfblocks();
        state.request_image_loads(&picker);
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            state.drain_image_loads(&picker);
            if !state.filtered_indices().is_empty() {
                break;
            }
        }

        assert_eq!(state.filtered_indices(), vec![0]);
    }

    #[test]
    fn streamed_entries_without_icons_are_visible_immediately() {
        let mut state = ContentListState::default();
        let stream = state.start_stream("local");

        assert!(stream.send(entry("Without icon")));
        state.drain_pending();

        assert_eq!(state.filtered_indices(), vec![0]);
    }

    #[test]
    fn manifest_metadata_keeps_an_embedded_icon_renderer() {
        let minecraft_dir = PathBuf::from("instance/minecraft");
        let mut state = ContentListState::default();
        let mut installed = entry("Installed");
        installed.path = minecraft_dir.join("mods/installed.jar");
        installed.icon_bytes = Some(vec![1, 2, 3]);
        state.entries.push(installed);
        let picker = ratatui_image::picker::Picker::halfblocks();
        state.image_protocols.insert(
            "installed".to_owned(),
            picker.new_resize_protocol(image::DynamicImage::new_rgba8(1, 1)),
        );
        let mut manifest = crate::instance::ContentManifest::default();
        manifest.upsert(crate::instance::ContentFileRecord {
            relative_path: PathBuf::from("mods/installed.jar"),
            kind: crate::instance::ContentKind::Mod,
            enabled: true,
            fingerprint: crate::instance::FileFingerprint {
                size: 3,
                modified_ns: 1,
                hashes: Default::default(),
            },
            resolution: crate::instance::Resolution::Resolved {
                project: crate::instance::ProviderProject {
                    provider: "modrinth".to_owned(),
                    project_id: "project".to_owned(),
                    version_id: "version".to_owned(),
                },
            },
        });

        state.apply_manifest(&manifest, &minecraft_dir, crate::instance::ContentKind::Mod);

        assert!(state.image_protocols.contains_key("installed"));
    }

    #[tokio::test]
    async fn provider_icons_load_from_cache_without_network() {
        let temp = tempfile::tempdir().unwrap();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let png = png.into_inner();
        let icon_path = crate::storage::MetadataPaths::new(temp.path())
            .provider_icons("modrinth")
            .join("cached-project.img");
        std::fs::create_dir_all(icon_path.parent().unwrap()).unwrap();
        std::fs::write(&icon_path, &png).unwrap();

        let bytes = load_provider_icon(
            &crate::net::HttpClient::new(),
            temp.path(),
            "modrinth",
            "cached-project",
        )
        .await
        .unwrap();

        assert_eq!(bytes, png);
    }
}
