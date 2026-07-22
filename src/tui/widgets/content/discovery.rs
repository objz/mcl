use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent};

use crate::instance::content::mods::ContentEntry;
use crate::instance::{InstanceConfig, ModLoader};
use crate::net::modrinth::{DiscoveryKind, DiscoveryProject};

use super::list::{ContentListState, ContentStream};

pub const PAGE_SIZE: usize = 20;
const PREFETCH_ITEMS: usize = 5;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ContentMode {
    #[default]
    Installed,
    Discover,
}

impl ContentMode {
    pub fn toggle(self) -> Self {
        match self {
            Self::Installed => Self::Discover,
            Self::Discover => Self::Installed,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Installed => "Installed",
            Self::Discover => "Discover",
        }
    }
}

pub struct PendingDiscoveryResult {
    generation: u64,
    offset: usize,
    result: Result<DiscoveryPageResult, String>,
}

pub struct DiscoveryPageResult {
    pub received: usize,
    pub total_hits: usize,
}

pub struct DiscoveryRequest {
    pub generation: u64,
    pub offset: usize,
    pub pending: PendingDiscovery,
    pub stream: ContentStream,
}

pub(crate) type PendingDiscovery = Arc<Mutex<Vec<PendingDiscoveryResult>>>;

pub struct DiscoveryState {
    pub kind: DiscoveryKind,
    pub list: ContentListState,
    pub search: crate::tui::widgets::search::SearchState,
    pub total_hits: usize,
    pub error: Option<String>,
    context: Option<String>,
    generation: u64,
    pending: PendingDiscovery,
    stream: Option<ContentStream>,
    next_offset: usize,
    page_loading: bool,
    exhausted: bool,
    viewport_rows: u16,
}

impl DiscoveryState {
    pub fn new(kind: DiscoveryKind) -> Self {
        Self {
            kind,
            list: ContentListState::default(),
            search: crate::tui::widgets::search::SearchState::default(),
            total_hits: 0,
            error: None,
            context: None,
            generation: 0,
            pending: Arc::new(Mutex::new(Vec::new())),
            stream: None,
            next_offset: 0,
            page_loading: false,
            exhausted: false,
            viewport_rows: 0,
        }
    }

    pub fn needs_search(&self, instance: &InstanceConfig) -> bool {
        self.context.as_deref() != Some(discovery_context(instance).as_str())
    }

    pub fn begin_search(&mut self, instance: &InstanceConfig) -> DiscoveryRequest {
        self.generation = self.generation.wrapping_add(1);
        let context = discovery_context(instance);
        self.context = Some(context.clone());
        self.total_hits = 0;
        self.error = None;
        self.next_offset = 0;
        self.page_loading = true;
        self.exhausted = false;
        let stream = self.list.start_source_stream(context);
        self.stream = Some(stream.clone());
        DiscoveryRequest {
            generation: self.generation,
            offset: 0,
            pending: self.pending.clone(),
            stream,
        }
    }

    pub fn begin_next_page(&mut self) -> Option<DiscoveryRequest> {
        if !self.should_load_more() {
            return None;
        }
        self.page_loading = true;
        Some(DiscoveryRequest {
            generation: self.generation,
            offset: self.next_offset,
            pending: self.pending.clone(),
            stream: self.stream.clone()?,
        })
    }

    pub fn set_viewport_rows(&mut self, rows: u16) {
        self.viewport_rows = rows;
    }

    pub fn push_result(
        pending: &PendingDiscovery,
        generation: u64,
        offset: usize,
        result: Result<DiscoveryPageResult, String>,
    ) {
        if let Ok(mut pending) = pending.lock() {
            pending.push(PendingDiscoveryResult {
                generation,
                offset,
                result,
            });
            crate::tui::request_redraw();
        }
    }

    pub fn drain_pending(&mut self) {
        let results = match self.pending.lock() {
            Ok(mut pending) => std::mem::take(&mut *pending),
            Err(_) => return,
        };
        for pending in results {
            if pending.generation != self.generation {
                continue;
            }
            self.page_loading = false;
            self.list.loading = false;
            match pending.result {
                Ok(result) => {
                    self.total_hits = result.total_hits;
                    self.next_offset = pending.offset.saturating_add(result.received);
                    self.exhausted = result.received == 0 || self.next_offset >= self.total_hits;
                    self.error = None;
                }
                Err(error) => {
                    self.total_hits = 0;
                    self.error = Some(error);
                    self.exhausted = true;
                }
            }
        }
    }

    pub fn empty_text(&self) -> &str {
        self.error
            .as_deref()
            .unwrap_or("No Modrinth projects found.")
    }

    fn should_load_more(&self) -> bool {
        if self.page_loading || self.exhausted || self.error.is_some() || self.stream.is_none() {
            return false;
        }
        let viewport_items = usize::from(self.viewport_rows).div_ceil(3);
        let selected = self.list.list_state.selected.unwrap_or(0);
        self.list.entries.len() < viewport_items.saturating_add(PREFETCH_ITEMS)
            || selected.saturating_add(PREFETCH_ITEMS) >= self.list.entries.len()
    }
}

pub fn handle_key(key_event: &KeyEvent, state: &mut DiscoveryState) -> (bool, bool) {
    if state.search.active {
        let refresh = key_event.code == KeyCode::Enter
            || (key_event.code == KeyCode::Esc && !state.search.query.is_empty());
        match key_event.code {
            KeyCode::Enter => state.search.confirm(),
            KeyCode::Esc => state.search.deactivate(),
            KeyCode::Backspace => state.search.pop(),
            KeyCode::Char(c) => state.search.push(c),
            _ => {}
        }
        return (true, refresh);
    }
    if key_event.code == KeyCode::Char('/') {
        state.search.activate();
        return (true, false);
    }
    (
        super::list::handle_key_no_toggle(key_event, &mut state.list),
        false,
    )
}

fn discovery_context(instance: &InstanceConfig) -> String {
    format!(
        "{}:{}:{}",
        instance.name,
        instance.game_version,
        loader_slug(instance.loader).unwrap_or("vanilla")
    )
}

fn loader_slug(loader: ModLoader) -> Option<&'static str> {
    match loader {
        ModLoader::Vanilla => None,
        ModLoader::Fabric => Some("fabric"),
        ModLoader::Forge => Some("forge"),
        ModLoader::NeoForge => Some("neoforge"),
        ModLoader::Quilt => Some("quilt"),
    }
}

pub(crate) fn project_entry(project: DiscoveryProject) -> ContentEntry {
    let description = format!(
        "{}  •  {} downloads",
        project.description,
        format_downloads(project.downloads)
    );
    ContentEntry {
        file_stem: project.id.clone(),
        name: project.title,
        description,
        enabled: true,
        icon_bytes: project.icon_bytes,
        path: PathBuf::from(project.id),
        icon_lines: Some(crate::instance::content::mods::fallback_icon()),
    }
}

fn format_downloads(downloads: u64) -> String {
    match downloads {
        1_000_000.. => format!("{:.1}M", downloads as f64 / 1_000_000.0),
        1_000.. => format!("{:.1}K", downloads as f64 / 1_000.0),
        _ => downloads.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn instance(name: &str, version: &str) -> InstanceConfig {
        InstanceConfig {
            name: name.to_string(),
            game_version: version.to_string(),
            loader: ModLoader::Fabric,
            loader_version: None,
            created: Utc::now(),
            last_played: None,
            java_path: None,
            memory_max: None,
            memory_min: None,
            jvm_args: Vec::new(),
            resolution: None,
            config_sync_profile: None,
        }
    }

    #[test]
    fn content_mode_toggles_both_ways() {
        assert_eq!(ContentMode::Installed.toggle(), ContentMode::Discover);
        assert_eq!(ContentMode::Discover.toggle(), ContentMode::Installed);
    }

    #[test]
    fn changing_instance_invalidates_results() {
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        let first = instance("one", "1.21.1");
        let second = instance("two", "1.21.1");
        let _request = state.begin_search(&first);

        assert!(!state.needs_search(&first));
        assert!(state.needs_search(&second));
    }

    #[test]
    fn stale_search_result_is_ignored() {
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        let instance = instance("one", "1.21.1");
        let old = state.begin_search(&instance);
        let _new = state.begin_search(&instance);
        DiscoveryState::push_result(
            &old.pending,
            old.generation,
            old.offset,
            Ok(DiscoveryPageResult {
                received: 20,
                total_hits: 99,
            }),
        );

        state.drain_pending();

        assert_eq!(state.total_hits, 0);
        assert!(state.list.loading);
    }

    #[test]
    fn next_page_prefetches_before_selection_reaches_the_end() {
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        let instance = instance("one", "1.21.1");
        let first = state.begin_search(&instance);
        for index in 0..PAGE_SIZE {
            assert!(first.stream.send(project_entry(DiscoveryProject {
                id: index.to_string(),
                slug: index.to_string(),
                title: index.to_string(),
                description: String::new(),
                downloads: 0,
                icon_url: None,
                icon_bytes: None,
            })));
        }
        state.list.drain_pending();
        DiscoveryState::push_result(
            &first.pending,
            first.generation,
            first.offset,
            Ok(DiscoveryPageResult {
                received: PAGE_SIZE,
                total_hits: 100,
            }),
        );
        state.drain_pending();

        state.list.list_state.selected = Some(PAGE_SIZE - PREFETCH_ITEMS);
        let next = state.begin_next_page().expect("next page should prefetch");
        assert_eq!(next.offset, PAGE_SIZE);
        assert!(state.begin_next_page().is_none());
    }

    #[test]
    fn tall_viewport_loads_enough_pages_to_fill_it() {
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        state.set_viewport_rows(90);
        let first = state.begin_search(&instance("one", "1.21.1"));
        for index in 0..PAGE_SIZE {
            assert!(first.stream.send(project_entry(DiscoveryProject {
                id: index.to_string(),
                slug: index.to_string(),
                title: index.to_string(),
                description: String::new(),
                downloads: 0,
                icon_url: None,
                icon_bytes: None,
            })));
        }
        state.list.drain_pending();
        DiscoveryState::push_result(
            &first.pending,
            first.generation,
            first.offset,
            Ok(DiscoveryPageResult {
                received: PAGE_SIZE,
                total_hits: 100,
            }),
        );
        state.drain_pending();

        assert_eq!(state.begin_next_page().unwrap().offset, PAGE_SIZE);
    }
}
