use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent};

use crate::instance::content::mods::ContentEntry;
use crate::instance::{InstanceConfig, ModLoader};
use crate::net::modrinth::{DiscoveryKind, DiscoveryProject};

use super::list::{ContentListState, ContentStream};

pub const PAGE_SIZE: usize = 100;
const PREFETCH_VIEWPORTS: usize = 2;
const MIN_PREFETCH_ITEMS: usize = 10;
const SEARCH_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(300);
const PAGE_RETRY_BASE_DELAY: std::time::Duration = std::time::Duration::from_millis(500);
const PAGE_RETRY_MAX_DELAY: std::time::Duration = std::time::Duration::from_secs(8);

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
            Self::Discover => "Discovery",
        }
    }
}

pub struct PendingDiscoveryResult {
    generation: u64,
    offset: usize,
    result: Result<DiscoveryPageResult, DiscoveryPageError>,
}

pub struct DiscoveryPageError {
    pub message: String,
    pub retryable: bool,
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
    pub reconcile: bool,
    pub loaded_icon_stems: std::collections::HashSet<String>,
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
    search_changed_at: Option<std::time::Instant>,
    retry_page_at: Option<std::time::Instant>,
    page_retry_attempt: u32,
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
            search_changed_at: None,
            retry_page_at: None,
            page_retry_attempt: 0,
        }
    }

    pub fn needs_search(&self, instance: &InstanceConfig) -> bool {
        self.context.as_deref() != Some(discovery_context(instance).as_str())
    }

    pub fn begin_search(&mut self, instance: &InstanceConfig) -> DiscoveryRequest {
        self.generation = self.generation.wrapping_add(1);
        let context = discovery_context(instance);
        let reconcile =
            self.context.as_deref() == Some(context.as_str()) && !self.list.entries.is_empty();
        self.context = Some(context.clone());
        self.total_hits = 0;
        self.error = None;
        self.next_offset = 0;
        self.page_loading = true;
        self.exhausted = false;
        self.search_changed_at = None;
        self.retry_page_at = None;
        self.page_retry_attempt = 0;
        let loaded_icon_stems = if reconcile {
            self.list
                .entries
                .iter()
                .filter(|entry| entry.icon_bytes.is_some())
                .map(|entry| entry.file_stem.clone())
                .collect()
        } else {
            std::collections::HashSet::new()
        };
        let stream = if reconcile {
            self.list.refresh_source_stream(context)
        } else {
            self.list.start_source_stream(context)
        };
        self.list.search.query.clone_from(&self.search.query);
        self.list.set_search_filtering(false);
        self.stream = Some(stream.clone());
        DiscoveryRequest {
            generation: self.generation,
            offset: 0,
            pending: self.pending.clone(),
            stream,
            reconcile,
            loaded_icon_stems,
        }
    }

    pub fn begin_next_page(&mut self) -> Option<DiscoveryRequest> {
        if !self.should_load_more() {
            return None;
        }
        self.page_loading = true;
        self.retry_page_at = None;
        let loaded_icon_stems = self
            .list
            .entries
            .iter()
            .filter(|entry| entry.icon_bytes.is_some())
            .map(|entry| entry.file_stem.clone())
            .collect();
        Some(DiscoveryRequest {
            generation: self.generation,
            offset: self.next_offset,
            pending: self.pending.clone(),
            stream: self.stream.clone()?,
            reconcile: false,
            loaded_icon_stems,
        })
    }

    pub fn set_viewport_rows(&mut self, rows: u16) {
        self.viewport_rows = rows;
    }

    pub fn search_due(&self) -> bool {
        self.search_changed_at
            .is_some_and(|changed| changed.elapsed() >= SEARCH_DEBOUNCE)
    }

    fn search_changed(&mut self) {
        self.list.search.query.clone_from(&self.search.query);
        self.list.set_search_filtering(false);
        self.search_changed_at = Some(std::time::Instant::now());
    }

    pub fn push_result(
        pending: &PendingDiscovery,
        generation: u64,
        offset: usize,
        result: Result<DiscoveryPageResult, DiscoveryPageError>,
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
                    self.retry_page_at = None;
                    self.page_retry_attempt = 0;
                }
                Err(error) => {
                    if error.retryable {
                        let multiplier = 1u32 << self.page_retry_attempt.min(4);
                        let delay = PAGE_RETRY_BASE_DELAY
                            .saturating_mul(multiplier)
                            .min(PAGE_RETRY_MAX_DELAY);
                        self.page_retry_attempt = self.page_retry_attempt.saturating_add(1);
                        self.retry_page_at = Some(std::time::Instant::now() + delay);
                        tracing::debug!(
                            "Modrinth discovery page at offset {} failed; retrying in {:?}: {}",
                            pending.offset,
                            delay,
                            error.message
                        );
                    } else if pending.offset == 0 {
                        self.total_hits = 0;
                        self.error = Some(error.message);
                        self.exhausted = true;
                    } else {
                        tracing::warn!(
                            "Modrinth discovery page at offset {} failed: {}",
                            pending.offset,
                            error.message
                        );
                        self.exhausted = true;
                    }
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
        if self.page_loading
            || self.exhausted
            || self.error.is_some()
            || self.stream.is_none()
            || self.search_changed_at.is_some()
            || self
                .retry_page_at
                .is_some_and(|retry_at| std::time::Instant::now() < retry_at)
        {
            return false;
        }
        let viewport_items = usize::from(self.viewport_rows).div_ceil(3);
        let prefetch_items = viewport_items
            .saturating_mul(PREFETCH_VIEWPORTS)
            .max(MIN_PREFETCH_ITEMS);
        let selected = self.list.list_state.selected.unwrap_or(0);
        self.list.entries.len() < viewport_items.saturating_add(prefetch_items)
            || selected.saturating_add(prefetch_items) >= self.list.entries.len()
    }
}

pub fn handle_key(key_event: &KeyEvent, state: &mut DiscoveryState) -> bool {
    if state.search.active {
        let previous_query = state.search.query.clone();
        match key_event.code {
            KeyCode::Enter => state.search.confirm(),
            KeyCode::Esc => state.search.deactivate(),
            KeyCode::Backspace => state.search.pop(),
            KeyCode::Char(c) => state.search.push(c),
            _ => {}
        }
        let changed = state.search.query != previous_query;
        if changed {
            state.search_changed();
        }
        return true;
    }
    if key_event.code == KeyCode::Char('/') {
        state.search.activate();
        return true;
    }
    super::list::handle_key_no_toggle(key_event, &mut state.list)
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

pub(crate) fn project_entry(project: DiscoveryProject, installed: bool) -> ContentEntry {
    ContentEntry {
        file_stem: project.id.clone(),
        name: project.title,
        title_suffix: installed.then(|| "Installed".to_owned()),
        footer_label: Some(format!("{} downloads", format_downloads(project.downloads))),
        description: project.description,
        enabled: true,
        icon_bytes: project.icon_bytes,
        path: PathBuf::from(project.id),
        icon_lines: Some(crate::instance::content::mods::fallback_icon()),
    }
}

pub(crate) fn project_is_installed(
    project: &DiscoveryProject,
    installed_entries: &[ContentEntry],
) -> bool {
    installed_entries.iter().any(|entry| {
        identity_matches(&entry.name, &project.title)
            || identity_matches(&entry.name, &project.slug)
            || filename_matches_project(&entry.file_stem, &project.slug)
            || filename_matches_project(&entry.file_stem, &project.title)
    })
}

fn identity_matches(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        value
            .chars()
            .filter(|character| character.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect::<String>()
    };
    let right = normalize(right);
    !right.is_empty() && normalize(left) == right
}

fn filename_matches_project(file_stem: &str, project_name: &str) -> bool {
    let file_stem = file_stem.to_lowercase();
    let project_name = project_name.to_lowercase().replace(' ', "-");
    file_stem == project_name
        || file_stem
            .strip_prefix(&project_name)
            .is_some_and(|suffix| suffix.starts_with(['-', '_', '.']))
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
    use crossterm::event::KeyModifiers;

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
    fn project_metadata_is_split_between_title_and_footer_badges() {
        let entry = project_entry(
            DiscoveryProject {
                id: "example".to_owned(),
                slug: "example".to_owned(),
                title: "Example".to_owned(),
                description: "Project description".to_owned(),
                downloads: 1_234,
                icon_url: None,
                icon_bytes: None,
            },
            true,
        );

        assert_eq!(entry.title_suffix.as_deref(), Some("Installed"));
        assert_eq!(entry.footer_label.as_deref(), Some("1.2K downloads"));
        assert_eq!(entry.description, "Project description");
    }

    #[test]
    fn installed_projects_match_metadata_names_and_versioned_filenames() {
        let project = DiscoveryProject {
            id: "P7dR8mSH".to_owned(),
            slug: "fabric-api".to_owned(),
            title: "Fabric API".to_owned(),
            description: String::new(),
            downloads: 0,
            icon_url: None,
            icon_bytes: None,
        };
        let mut local = project_entry(project.clone(), false);
        local.name = "Fabric API".to_owned();
        local.file_stem = "unrelated-file".to_owned();
        assert!(project_is_installed(&project, &[local]));

        let mut local = project_entry(project.clone(), false);
        local.name = "Unknown".to_owned();
        local.file_stem = "fabric-api-0.116.0+1.21.1".to_owned();
        assert!(project_is_installed(&project, &[local]));

        let mut local = project_entry(project.clone(), false);
        local.name = "Fabric Language Kotlin".to_owned();
        local.file_stem = "fabric-language-kotlin".to_owned();
        assert!(!project_is_installed(&project, &[local]));
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
    fn changing_instance_compatibility_invalidates_results() {
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        let original = instance("one", "1.21.1");
        let mut other_version = original.clone();
        other_version.game_version = "1.20.1".to_owned();
        let mut other_loader = original.clone();
        other_loader.loader = ModLoader::NeoForge;
        let _request = state.begin_search(&original);

        assert!(state.needs_search(&other_version));
        assert!(state.needs_search(&other_loader));
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
        state.set_viewport_rows(30);
        let instance = instance("one", "1.21.1");
        let first = state.begin_search(&instance);
        for index in 0..PAGE_SIZE {
            assert!(first.stream.send(project_entry(
                DiscoveryProject {
                    id: index.to_string(),
                    slug: index.to_string(),
                    title: index.to_string(),
                    description: String::new(),
                    downloads: 0,
                    icon_url: None,
                    icon_bytes: None,
                },
                false
            )));
        }
        state.list.drain_pending();
        DiscoveryState::push_result(
            &first.pending,
            first.generation,
            first.offset,
            Ok(DiscoveryPageResult {
                received: PAGE_SIZE,
                total_hits: 300,
            }),
        );
        state.drain_pending();
        state.list.list_state.selected = Some(80);

        let next = state.begin_next_page().expect("next page should prefetch");
        assert_eq!(next.offset, PAGE_SIZE);
        assert!(state.begin_next_page().is_none());
    }

    #[test]
    fn large_page_fills_a_tall_viewport_without_another_request() {
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        state.set_viewport_rows(90);
        let first = state.begin_search(&instance("one", "1.21.1"));
        for index in 0..PAGE_SIZE {
            assert!(first.stream.send(project_entry(
                DiscoveryProject {
                    id: index.to_string(),
                    slug: index.to_string(),
                    title: index.to_string(),
                    description: String::new(),
                    downloads: 0,
                    icon_url: None,
                    icon_bytes: None,
                },
                false
            )));
        }
        state.list.drain_pending();
        DiscoveryState::push_result(
            &first.pending,
            first.generation,
            first.offset,
            Ok(DiscoveryPageResult {
                received: PAGE_SIZE,
                total_hits: 300,
            }),
        );
        state.drain_pending();

        assert!(state.begin_next_page().is_none());
    }

    #[test]
    fn typing_keeps_loaded_results_until_remote_search_is_due() {
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        let request = state.begin_search(&instance("one", "1.21.1"));
        for title in ["Sodium", "Lithium"] {
            assert!(request.stream.send(project_entry(
                DiscoveryProject {
                    id: title.to_lowercase(),
                    slug: title.to_lowercase(),
                    title: title.to_owned(),
                    description: String::new(),
                    downloads: 0,
                    icon_url: None,
                    icon_bytes: None,
                },
                false
            )));
        }
        state.list.drain_pending();

        handle_key(
            &KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
            &mut state,
        );
        for character in "sod".chars() {
            handle_key(
                &KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE),
                &mut state,
            );
        }

        assert_eq!(state.list.filtered_indices(), vec![0, 1]);
        assert_eq!(state.list.search.query, "sod");
        assert!(!state.search_due());
        state.search_changed_at = Some(std::time::Instant::now() - SEARCH_DEBOUNCE);
        assert!(state.search_due());
    }

    #[test]
    fn search_refresh_keeps_rows_until_the_diff_arrives() {
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        let instance = instance("one", "1.21.1");
        let initial = state.begin_search(&instance);
        for title in ["Sodium", "Lithium"] {
            assert!(initial.stream.upsert(project_entry(
                DiscoveryProject {
                    id: title.to_lowercase(),
                    slug: title.to_lowercase(),
                    title: title.to_owned(),
                    description: String::new(),
                    downloads: 0,
                    icon_url: None,
                    icon_bytes: (title == "Sodium").then(|| vec![1, 2, 3]),
                },
                false
            )));
        }
        state.list.drain_pending();
        state.search.query = "sodium".to_owned();
        state.search_changed();

        let refresh = state.begin_search(&instance);

        assert!(refresh.reconcile);
        assert!(refresh.loaded_icon_stems.contains("sodium"));
        assert!(!refresh.loaded_icon_stems.contains("lithium"));
        assert_eq!(state.list.entries.len(), 2);
        assert!(!state.list.loading);
    }

    #[test]
    fn pagination_continues_across_multiple_pages() {
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        let instance = instance("one", "1.21.1");
        let first = state.begin_search(&instance);
        for index in 0..PAGE_SIZE {
            assert!(first.stream.upsert(project_entry(
                DiscoveryProject {
                    id: index.to_string(),
                    slug: index.to_string(),
                    title: index.to_string(),
                    description: String::new(),
                    downloads: 0,
                    icon_url: None,
                    icon_bytes: None,
                },
                false
            )));
        }
        state.list.drain_pending();
        DiscoveryState::push_result(
            &first.pending,
            first.generation,
            first.offset,
            Ok(DiscoveryPageResult {
                received: PAGE_SIZE,
                total_hits: 300,
            }),
        );
        state.drain_pending();
        state.list.list_state.selected = Some(PAGE_SIZE - MIN_PREFETCH_ITEMS);

        let second = state.begin_next_page().unwrap();
        for index in PAGE_SIZE..PAGE_SIZE * 2 {
            assert!(second.stream.upsert(project_entry(
                DiscoveryProject {
                    id: index.to_string(),
                    slug: index.to_string(),
                    title: index.to_string(),
                    description: String::new(),
                    downloads: 0,
                    icon_url: None,
                    icon_bytes: None,
                },
                false
            )));
        }
        state.list.drain_pending();
        DiscoveryState::push_result(
            &second.pending,
            second.generation,
            second.offset,
            Ok(DiscoveryPageResult {
                received: PAGE_SIZE,
                total_hits: 300,
            }),
        );
        state.drain_pending();
        state.list.list_state.selected = Some(PAGE_SIZE * 2 - MIN_PREFETCH_ITEMS);

        let third = state.begin_next_page().unwrap();
        assert_eq!(third.offset, PAGE_SIZE * 2);
    }

    #[test]
    fn permanent_pagination_failure_stops_without_discarding_loaded_entries() {
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        let first = state.begin_search(&instance("one", "1.21.1"));
        for index in 0..PAGE_SIZE {
            assert!(first.stream.upsert(project_entry(
                DiscoveryProject {
                    id: index.to_string(),
                    slug: index.to_string(),
                    title: index.to_string(),
                    description: String::new(),
                    downloads: 0,
                    icon_url: None,
                    icon_bytes: None,
                },
                false
            )));
        }
        state.list.drain_pending();
        DiscoveryState::push_result(
            &first.pending,
            first.generation,
            first.offset,
            Ok(DiscoveryPageResult {
                received: PAGE_SIZE,
                total_hits: 300,
            }),
        );
        state.drain_pending();
        state.list.list_state.selected = Some(PAGE_SIZE - MIN_PREFETCH_ITEMS);
        let second = state.begin_next_page().unwrap();
        DiscoveryState::push_result(
            &second.pending,
            second.generation,
            second.offset,
            Err(DiscoveryPageError {
                message: "invalid response".to_owned(),
                retryable: false,
            }),
        );
        state.drain_pending();

        assert!(state.begin_next_page().is_none());
        assert_eq!(state.list.entries.len(), PAGE_SIZE);
        assert!(state.exhausted);
    }

    #[test]
    fn transient_pagination_failure_retries_the_same_offset_after_a_delay() {
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        let first = state.begin_search(&instance("one", "1.21.1"));
        DiscoveryState::push_result(
            &first.pending,
            first.generation,
            first.offset,
            Ok(DiscoveryPageResult {
                received: PAGE_SIZE,
                total_hits: 300,
            }),
        );
        state.drain_pending();

        let second = state.begin_next_page().unwrap();
        DiscoveryState::push_result(
            &second.pending,
            second.generation,
            second.offset,
            Err(DiscoveryPageError {
                message: "connection reset".to_owned(),
                retryable: true,
            }),
        );
        state.drain_pending();

        assert!(state.begin_next_page().is_none());
        state.retry_page_at = Some(std::time::Instant::now() - PAGE_RETRY_BASE_DELAY);
        assert_eq!(state.begin_next_page().unwrap().offset, PAGE_SIZE);
    }
}
