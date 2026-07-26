use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent};

use crate::instance::content::mods::ContentEntry;
use crate::instance::{InstanceConfig, ModLoader};
use crate::net::modrinth::{DiscoveryKind, DiscoveryProject, VersionInfo};

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

pub struct VersionPopupState {
    request_id: u64,
    pub project_id: String,
    pub project_title: String,
    pub installed_path: Option<PathBuf>,
    pub versions: Vec<VersionInfo>,
    pub selected: usize,
    pub loading: bool,
    pub confirming: bool,
    pub installing: bool,
    pub error: Option<String>,
}

impl VersionPopupState {
    pub fn title(&self) -> String {
        if self.installed_path.is_some() {
            format!("Change {} version", self.project_title)
        } else {
            format!("Install {}", self.project_title)
        }
    }
}

pub struct VersionsRequest {
    pub request_id: u64,
    pub project_id: String,
    pub pending: PendingActions,
}

pub struct ProjectPageRequest {
    pub request_id: u64,
    pub project_id: String,
    pub project_title: String,
    pub cached_project: Option<crate::net::modrinth::ProjectInfo>,
    pub image_urls: Vec<String>,
    pub pending: PendingActions,
}

pub struct ProjectPageState {
    request_id: u64,
    project_id: String,
    pub title: String,
    pub document: Option<crate::tui::widgets::markdown::Document>,
    pub error: Option<String>,
    pub scroll: usize,
    pub max_scroll: usize,
}

pub struct InstallRequest {
    pub request_id: u64,
    pub generation: u64,
    pub project_id: String,
    pub project_title: String,
    pub version: VersionInfo,
    pub installed_path: Option<PathBuf>,
    pub pending: PendingActions,
}

pub struct InstallCompletion {
    pub path: PathBuf,
    pub replaced: bool,
    pub skipped: bool,
}

pub enum DiscoveryActionResult {
    ProjectPage {
        request_id: u64,
        project_id: String,
        result: Result<crate::net::modrinth::ProjectInfo, String>,
    },
    ProjectImage {
        request_id: u64,
        project_id: String,
        url: String,
        result: Result<image::DynamicImage, String>,
    },
    Versions {
        request_id: u64,
        project_id: String,
        result: Result<Vec<VersionInfo>, String>,
    },
    Install {
        request_id: u64,
        generation: u64,
        project_id: String,
        project_title: String,
        result: Result<InstallCompletion, String>,
    },
}

pub(crate) type PendingActions = Arc<Mutex<Vec<DiscoveryActionResult>>>;

pub struct DiscoveryState {
    pub kind: DiscoveryKind,
    pub list: ContentListState,
    pub search: crate::tui::widgets::search::SearchState,
    pub total_hits: usize,
    pub error: Option<String>,
    context: Option<String>,
    generation: u64,
    pending: PendingDiscovery,
    pending_actions: PendingActions,
    project_pages: std::collections::HashMap<String, crate::net::modrinth::ProjectInfo>,
    project_images: std::collections::HashMap<(String, String), image::DynamicImage>,
    pub project_page: Option<ProjectPageState>,
    pub version_popup: Option<VersionPopupState>,
    next_action_request_id: u64,
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
            pending_actions: Arc::new(Mutex::new(Vec::new())),
            project_pages: std::collections::HashMap::new(),
            project_images: std::collections::HashMap::new(),
            project_page: None,
            version_popup: None,
            next_action_request_id: 0,
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

    pub fn unavailable_message(&self, instance: &InstanceConfig) -> Option<&'static str> {
        self.kind.unavailable_message(instance.loader)
    }

    pub fn set_unavailable(&mut self, instance: &InstanceConfig) {
        let context = discovery_context(instance);
        if self.context.as_deref() == Some(&context) && self.list.entries.is_empty() {
            return;
        }
        self.context = None;
        drop(self.begin_search(instance));
        self.stream = None;
        self.page_loading = false;
        self.exhausted = true;
    }

    pub fn begin_search(&mut self, instance: &InstanceConfig) -> DiscoveryRequest {
        self.generation = self.generation.wrapping_add(1);
        self.project_page = None;
        self.version_popup = None;
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

    pub fn begin_versions(&mut self) -> Option<VersionsRequest> {
        let filtered = self.list.filtered_indices();
        let index = self
            .list
            .list_state
            .selected
            .and_then(|selected| filtered.get(selected))?;
        let entry = self.list.entries.get(*index)?;
        let installed_path = entry.installed_path.clone();
        let project_id = entry.file_stem.clone();
        self.next_action_request_id = self.next_action_request_id.wrapping_add(1);
        let request_id = self.next_action_request_id;
        self.version_popup = Some(VersionPopupState {
            request_id,
            project_id: project_id.clone(),
            project_title: entry.name.clone(),
            installed_path,
            versions: Vec::new(),
            selected: 0,
            loading: true,
            confirming: false,
            installing: false,
            error: None,
        });
        Some(VersionsRequest {
            request_id,
            project_id,
            pending: self.pending_actions.clone(),
        })
    }

    pub fn begin_project_page(&mut self) -> Option<ProjectPageRequest> {
        let filtered = self.list.filtered_indices();
        let index = self
            .list
            .list_state
            .selected
            .and_then(|selected| filtered.get(selected))?;
        let entry = self.list.entries.get(*index)?;
        let project_id = entry.file_stem.clone();
        let project_title = entry.name.clone();
        self.next_action_request_id = self.next_action_request_id.wrapping_add(1);
        let request_id = self.next_action_request_id;
        let cached = self.project_pages.get(&project_id);
        let mut document = cached.map(|project| {
            crate::tui::widgets::markdown::Document::new(&project.title, &project.body)
        });
        if let Some(document) = document.as_mut() {
            for url in document.image_urls() {
                if let Some(image) = self.project_images.get(&(project_id.clone(), url.clone())) {
                    document.set_image(&url, Ok(image.clone()));
                }
            }
        }
        let image_urls = document
            .as_ref()
            .map(crate::tui::widgets::markdown::Document::image_urls)
            .unwrap_or_default()
            .into_iter()
            .filter(|url| {
                !self
                    .project_images
                    .contains_key(&(project_id.clone(), url.clone()))
            })
            .collect::<Vec<_>>();
        self.project_page = Some(ProjectPageState {
            request_id,
            project_id: project_id.clone(),
            title: cached
                .map(|project| project.title.clone())
                .unwrap_or_else(|| project_title.clone()),
            document,
            error: None,
            scroll: 0,
            max_scroll: 0,
        });
        if cached.is_some() && image_urls.is_empty() {
            return None;
        }
        Some(ProjectPageRequest {
            request_id,
            project_id,
            project_title,
            cached_project: cached.cloned(),
            image_urls,
            pending: self.pending_actions.clone(),
        })
    }

    pub fn project_page_open(&self) -> bool {
        self.project_page.is_some()
    }

    pub fn refresh_installed_manifest(
        &mut self,
        manifest: &crate::instance::ContentManifest,
        minecraft_dir: &std::path::Path,
    ) {
        let mut changed = false;
        for entry in &mut self.list.entries {
            let Some(project) = entry.provider_project.as_ref() else {
                continue;
            };
            let installed_path = manifest.resolved_project_path(
                &project.provider,
                &project.project_id,
                minecraft_dir,
            );
            if entry.installed_path != installed_path {
                entry.title_suffix = installed_path.is_some().then(|| "Installed".to_owned());
                entry.installed_path = installed_path;
                changed = true;
            }
        }
        if changed {
            crate::tui::request_redraw();
        }
    }

    pub fn selected_is_installed(&self) -> bool {
        self.selected_installed_entry().is_some()
    }

    pub fn pending_installed_delete(
        &self,
    ) -> Option<crate::tui::widgets::content::list::PendingContentDelete> {
        let entry = self.selected_installed_entry()?;
        Some(crate::tui::widgets::content::list::PendingContentDelete {
            name: entry.name.clone(),
            path: entry.installed_path.clone()?,
        })
    }

    pub fn clear_installed_path(&mut self, path: &std::path::Path) -> bool {
        let Some(entry) = self
            .list
            .entries
            .iter_mut()
            .find(|entry| entry.installed_path.as_deref() == Some(path))
        else {
            return false;
        };
        entry.installed_path = None;
        entry.title_suffix = None;
        crate::tui::request_redraw();
        true
    }

    fn selected_installed_entry(&self) -> Option<&ContentEntry> {
        let filtered = self.list.filtered_indices();
        let index = self
            .list
            .list_state
            .selected
            .and_then(|selected| filtered.get(selected))?;
        self.list
            .entries
            .get(*index)
            .filter(|entry| entry.installed_path.is_some())
    }

    pub fn begin_install(&mut self) -> Option<InstallRequest> {
        let popup = self.version_popup.as_ref()?;
        if popup.loading || popup.installing || !popup.confirming {
            return None;
        }
        let version = popup.versions.get(popup.selected)?.clone();
        let request = InstallRequest {
            request_id: popup.request_id,
            generation: self.generation,
            project_id: popup.project_id.clone(),
            project_title: popup.project_title.clone(),
            version,
            installed_path: popup.installed_path.clone(),
            pending: self.pending_actions.clone(),
        };
        self.version_popup = None;
        Some(request)
    }

    pub fn begin_confirmation(&mut self) -> bool {
        let Some(popup) = self.version_popup.as_mut() else {
            return false;
        };
        if popup.loading || popup.installing || popup.versions.get(popup.selected).is_none() {
            return false;
        }
        popup.confirming = true;
        popup.error = None;
        true
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

    pub fn push_action_result(pending: &PendingActions, result: DiscoveryActionResult) {
        if let Ok(mut pending) = pending.lock() {
            pending.push(result);
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

        self.drain_action_results();
    }

    fn drain_action_results(&mut self) {
        let results = match self.pending_actions.lock() {
            Ok(mut pending) => std::mem::take(&mut *pending),
            Err(_) => return,
        };
        for result in results {
            match result {
                DiscoveryActionResult::ProjectPage {
                    request_id,
                    project_id,
                    result,
                } => {
                    let Some(page) = self.project_page.as_mut().filter(|page| {
                        page.request_id == request_id && page.project_id == project_id
                    }) else {
                        continue;
                    };
                    match result {
                        Ok(project) => {
                            page.title.clone_from(&project.title);
                            page.document = Some(crate::tui::widgets::markdown::Document::new(
                                &project.title,
                                &project.body,
                            ));
                            if let Some(document) = page.document.as_mut() {
                                for url in document.image_urls() {
                                    if let Some(image) =
                                        self.project_images.get(&(project_id.clone(), url.clone()))
                                    {
                                        document.set_image(&url, Ok(image.clone()));
                                    }
                                }
                            }
                            page.error = None;
                            page.scroll = 0;
                            page.max_scroll = 0;
                            self.project_pages.insert(project.id.clone(), project);
                        }
                        Err(error) => page.error = Some(error),
                    }
                }
                DiscoveryActionResult::ProjectImage {
                    request_id,
                    project_id,
                    url,
                    result,
                } => {
                    if let Ok(image) = &result {
                        self.project_images
                            .insert((project_id.clone(), url.clone()), image.clone());
                    }
                    let Some(page) = self.project_page.as_mut().filter(|page| {
                        page.request_id == request_id && page.project_id == project_id
                    }) else {
                        continue;
                    };
                    if let Some(document) = page.document.as_mut() {
                        document.set_image(&url, result);
                    }
                }
                DiscoveryActionResult::Versions {
                    request_id,
                    project_id,
                    result,
                } => {
                    let Some(popup) = self.version_popup.as_mut().filter(|popup| {
                        popup.request_id == request_id && popup.project_id == project_id
                    }) else {
                        continue;
                    };
                    popup.loading = false;
                    match result {
                        Ok(versions) => {
                            popup.versions = versions;
                            popup.selected = 0;
                            popup.error = None;
                        }
                        Err(error) => popup.error = Some(error),
                    }
                }
                DiscoveryActionResult::Install {
                    request_id,
                    generation,
                    project_id,
                    project_title,
                    result,
                } => match result {
                    Ok(completion) => {
                        if generation == self.generation
                            && let Some(entry) = self
                                .list
                                .entries
                                .iter_mut()
                                .find(|entry| entry.file_stem == project_id)
                        {
                            entry.title_suffix = Some("Installed".to_owned());
                            entry.installed_path = Some(completion.path.clone());
                        }
                        let action = if completion.skipped {
                            "already installed"
                        } else if completion.replaced {
                            "version changed"
                        } else {
                            "installed"
                        };
                        crate::tui::error_buffer::push_error(
                            crate::tui::error_buffer::ErrorEvent {
                                id: request_id,
                                level: tracing::Level::INFO,
                                message: format!("{project_title}: {action}"),
                                pushed_at: std::time::Instant::now(),
                            },
                        );
                    }
                    Err(error) => {
                        crate::tui::error_buffer::push_error(
                            crate::tui::error_buffer::ErrorEvent {
                                id: request_id,
                                level: tracing::Level::ERROR,
                                message: format!("{project_title}: {error}"),
                                pushed_at: std::time::Instant::now(),
                            },
                        );
                    }
                },
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
    if let Some(page) = state.project_page.as_mut() {
        match key_event.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => state.project_page = None,
            KeyCode::Char('j') | KeyCode::Down => {
                page.scroll = page.scroll.saturating_add(1).min(page.max_scroll);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                page.scroll = page.scroll.saturating_sub(1);
            }
            KeyCode::PageDown | KeyCode::Char('d') => {
                page.scroll = page.scroll.saturating_add(10).min(page.max_scroll);
            }
            KeyCode::PageUp | KeyCode::Char('u') => {
                page.scroll = page.scroll.saturating_sub(10);
            }
            KeyCode::Char('g') | KeyCode::Home => page.scroll = 0,
            KeyCode::Char('G') | KeyCode::End => page.scroll = page.max_scroll,
            _ => {}
        }
        return true;
    }
    if let Some(popup) = state.version_popup.as_mut() {
        if popup.confirming
            && matches!(key_event.code, KeyCode::Left | KeyCode::Char('h'))
            && !popup.installing
        {
            popup.confirming = false;
            popup.error = None;
            return true;
        }
        match key_event.code {
            KeyCode::Esc if !popup.installing => state.version_popup = None,
            KeyCode::Char('j') | KeyCode::Down
                if !popup.loading && !popup.installing && !popup.confirming =>
            {
                if popup.selected + 1 < popup.versions.len() {
                    popup.selected += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up
                if !popup.loading && !popup.installing && !popup.confirming =>
            {
                popup.selected = popup.selected.saturating_sub(1);
            }
            _ => {}
        }
        return true;
    }
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

pub(crate) fn project_entry(
    project: DiscoveryProject,
    installed_path: Option<PathBuf>,
) -> ContentEntry {
    let provider_icon = project.icon_bytes.is_some()
        || project
            .icon_url
            .as_deref()
            .is_some_and(|url| !url.trim().is_empty());
    ContentEntry {
        file_stem: project.id.clone(),
        name: project.title,
        source_slug: Some(project.slug),
        installed_path: installed_path.clone(),
        provider_project: Some(crate::instance::ProviderProject {
            provider: "modrinth".to_owned(),
            project_id: project.id.clone(),
            version_id: String::new(),
        }),
        title_suffix: installed_path.is_some().then(|| "Installed".to_owned()),
        footer_label: Some(format!("{} downloads", format_downloads(project.downloads))),
        description: project.description,
        enabled: true,
        icon_bytes: project.icon_bytes,
        provider_icon,
        provider_description: false,
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

    fn version(id: &str) -> VersionInfo {
        VersionInfo {
            id: id.to_owned(),
            project_id: "project".to_owned(),
            name: format!("Version {id}"),
            version_number: id.to_owned(),
            game_versions: vec!["1.21.1".to_owned()],
            loaders: vec!["fabric".to_owned()],
            date_published: "2026-01-02T12:00:00Z".to_owned(),
            files: Vec::new(),
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
            Some(PathBuf::from("example.jar")),
        );

        assert_eq!(entry.title_suffix.as_deref(), Some("Installed"));
        assert_eq!(entry.footer_label.as_deref(), Some("1.2K downloads"));
        assert_eq!(entry.description, "Project description");
        assert_eq!(entry.path, PathBuf::from("example"));
        assert_eq!(entry.installed_path, Some(PathBuf::from("example.jar")));
    }

    #[test]
    fn install_and_change_version_popups_only_differ_in_title() {
        let project = DiscoveryProject {
            id: "project".to_owned(),
            slug: "project".to_owned(),
            title: "Project".to_owned(),
            description: String::new(),
            downloads: 0,
            icon_url: None,
            icon_bytes: None,
        };
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        state
            .list
            .entries
            .push(project_entry(project.clone(), None));
        state.list.list_state.selected = Some(0);
        state.begin_versions().unwrap();
        assert_eq!(
            state.version_popup.as_ref().unwrap().title(),
            "Install Project"
        );

        state.version_popup = None;
        state.list.entries[0] = project_entry(project, Some(PathBuf::from("mods/project.jar")));
        state.begin_versions().unwrap();
        assert_eq!(
            state.version_popup.as_ref().unwrap().title(),
            "Change Project version"
        );
    }

    #[test]
    fn compatible_versions_populate_the_open_popup() {
        let project = DiscoveryProject {
            id: "project".to_owned(),
            slug: "project".to_owned(),
            title: "Project".to_owned(),
            description: String::new(),
            downloads: 0,
            icon_url: None,
            icon_bytes: None,
        };
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        state.list.entries.push(project_entry(project, None));
        state.list.list_state.selected = Some(0);
        let request = state.begin_versions().unwrap();
        DiscoveryState::push_action_result(
            &request.pending,
            DiscoveryActionResult::Versions {
                request_id: request.request_id,
                project_id: request.project_id,
                result: Ok(vec![version("1.0.0"), version("1.1.0")]),
            },
        );

        state.drain_pending();

        let popup = state.version_popup.as_ref().unwrap();
        assert!(!popup.loading);
        assert_eq!(popup.versions.len(), 2);
        assert_eq!(popup.selected, 0);
    }

    #[test]
    fn project_page_loads_for_the_selected_discovery_entry() {
        let project = DiscoveryProject {
            id: "project".to_owned(),
            slug: "project".to_owned(),
            title: "Project".to_owned(),
            description: String::new(),
            downloads: 0,
            icon_url: None,
            icon_bytes: None,
        };
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        state.list.entries.push(project_entry(project, None));
        state.list.list_state.selected = Some(0);

        let request = state.begin_project_page().unwrap();
        assert!(state.project_page_open());
        DiscoveryState::push_action_result(
            &request.pending,
            DiscoveryActionResult::ProjectPage {
                request_id: request.request_id,
                project_id: request.project_id,
                result: Ok(crate::net::modrinth::ProjectInfo {
                    id: "project".to_owned(),
                    slug: "project".to_owned(),
                    title: "Project page".to_owned(),
                    description: "Short description".to_owned(),
                    body: "Long **Markdown** description.".to_owned(),
                    icon_url: None,
                }),
            },
        );

        state.drain_pending();
        let page = state.project_page.as_ref().unwrap();
        assert_eq!(page.title, "Project page");
        assert!(page.document.is_some());
        state.project_page = None;
        assert!(state.begin_project_page().is_none());
        assert!(
            state
                .project_page
                .as_ref()
                .is_some_and(|page| page.document.is_some())
        );
    }

    #[test]
    fn project_page_navigation_is_bounded_and_can_go_back() {
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        state.project_page = Some(ProjectPageState {
            request_id: 1,
            project_id: "project".to_owned(),
            title: "Project".to_owned(),
            document: Some(crate::tui::widgets::markdown::Document::new(
                "Project", "Body",
            )),
            error: None,
            scroll: 0,
            max_scroll: 20,
        });

        handle_key(
            &KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
            &mut state,
        );
        assert_eq!(state.project_page.as_ref().unwrap().scroll, 10);
        handle_key(
            &KeyEvent::new(KeyCode::Char('G'), KeyModifiers::NONE),
            &mut state,
        );
        assert_eq!(state.project_page.as_ref().unwrap().scroll, 20);
        handle_key(
            &KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
            &mut state,
        );
        assert!(!state.project_page_open());
    }

    #[test]
    fn confirmation_can_return_to_version_selection() {
        let project = DiscoveryProject {
            id: "project".to_owned(),
            slug: "project".to_owned(),
            title: "Project".to_owned(),
            description: String::new(),
            downloads: 0,
            icon_url: None,
            icon_bytes: None,
        };
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        state.list.entries.push(project_entry(project, None));
        state.list.list_state.selected = Some(0);
        let request = state.begin_versions().unwrap();
        DiscoveryState::push_action_result(
            &request.pending,
            DiscoveryActionResult::Versions {
                request_id: request.request_id,
                project_id: request.project_id,
                result: Ok(vec![version("1.0.0")]),
            },
        );
        state.drain_pending();
        assert!(state.begin_confirmation());

        handle_key(
            &KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
            &mut state,
        );

        assert!(!state.version_popup.as_ref().unwrap().confirming);
        assert!(state.version_popup.is_some());
    }

    #[test]
    fn discovery_delete_only_clears_the_matching_installed_badge() {
        let first_path = PathBuf::from("mods/first.jar");
        let second_path = PathBuf::from("mods/second.jar");
        let project = |id: &str| DiscoveryProject {
            id: id.to_owned(),
            slug: id.to_owned(),
            title: id.to_owned(),
            description: String::new(),
            downloads: 0,
            icon_url: None,
            icon_bytes: None,
        };
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        state
            .list
            .entries
            .push(project_entry(project("first"), Some(first_path.clone())));
        state
            .list
            .entries
            .push(project_entry(project("second"), Some(second_path.clone())));
        state.list.list_state.selected = Some(0);

        let pending = state.pending_installed_delete().unwrap();
        assert_eq!(pending.path, first_path);
        assert!(state.clear_installed_path(&pending.path));

        assert_eq!(state.list.entries.len(), 2);
        assert!(state.list.entries[0].installed_path.is_none());
        assert!(state.list.entries[0].title_suffix.is_none());
        assert_eq!(
            state.list.entries[1].installed_path.as_deref(),
            Some(second_path.as_path())
        );
        assert_eq!(
            state.list.entries[1].title_suffix.as_deref(),
            Some("Installed")
        );
        assert!(state.pending_installed_delete().is_none());
    }

    #[test]
    fn successful_install_marks_the_project_and_closes_the_popup() {
        let project = DiscoveryProject {
            id: "project".to_owned(),
            slug: "project".to_owned(),
            title: "Project".to_owned(),
            description: String::new(),
            downloads: 0,
            icon_url: None,
            icon_bytes: None,
        };
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        state.list.entries.push(project_entry(project, None));
        state.list.list_state.selected = Some(0);
        let versions_request = state.begin_versions().unwrap();
        DiscoveryState::push_action_result(
            &versions_request.pending,
            DiscoveryActionResult::Versions {
                request_id: versions_request.request_id,
                project_id: versions_request.project_id,
                result: Ok(vec![version("1.0.0")]),
            },
        );
        state.drain_pending();
        assert!(state.begin_confirmation());
        let install = state.begin_install().unwrap();
        assert!(state.version_popup.is_none());
        DiscoveryState::push_action_result(
            &install.pending,
            DiscoveryActionResult::Install {
                request_id: install.request_id,
                generation: install.generation,
                project_id: install.project_id,
                project_title: install.project_title,
                result: Ok(InstallCompletion {
                    path: PathBuf::from("mods/project.jar"),
                    replaced: false,
                    skipped: false,
                }),
            },
        );

        state.drain_pending();
        assert!(state.version_popup.is_none());
        assert_eq!(
            state.list.entries[0].title_suffix.as_deref(),
            Some("Installed")
        );
        assert_eq!(
            state.list.entries[0].installed_path,
            Some(PathBuf::from("mods/project.jar"))
        );
        assert_eq!(state.list.entries[0].path, PathBuf::from("project"));
    }

    #[test]
    fn installed_labels_follow_exact_manifest_projects() {
        let project = DiscoveryProject {
            id: "project-id".to_owned(),
            slug: "example-project".to_owned(),
            title: "Example Project".to_owned(),
            description: String::new(),
            downloads: 0,
            icon_url: None,
            icon_bytes: None,
        };
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        state.list.entries.push(project_entry(project, None));

        let mut manifest = crate::instance::ContentManifest::default();
        manifest.upsert(crate::instance::ContentFileRecord {
            relative_path: PathBuf::from("mods/example-project-1.0.0.jar"),
            kind: crate::instance::ContentKind::Mod,
            enabled: true,
            fingerprint: crate::instance::FileFingerprint {
                size: 1,
                modified_ns: 1,
                hashes: Default::default(),
            },
            resolution: crate::instance::Resolution::Resolved {
                project: crate::instance::ProviderProject {
                    provider: "modrinth".to_owned(),
                    project_id: "project-id".to_owned(),
                    version_id: "version".to_owned(),
                },
            },
        });
        state.refresh_installed_manifest(&manifest, std::path::Path::new("first"));
        assert_eq!(
            state.list.entries[0].title_suffix.as_deref(),
            Some("Installed")
        );
        assert_eq!(
            state.list.entries[0].installed_path,
            Some(PathBuf::from("first/mods/example-project-1.0.0.jar"))
        );

        state.refresh_installed_manifest(
            &crate::instance::ContentManifest::default(),
            std::path::Path::new("first"),
        );
        assert_eq!(state.list.entries[0].title_suffix, None);
        assert_eq!(state.list.entries[0].installed_path, None);
        assert_eq!(state.list.entries[0].path, PathBuf::from("project-id"));
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
    fn unavailable_vanilla_discovery_clears_cached_results() {
        let mut state = DiscoveryState::new(DiscoveryKind::Mod);
        state.list.entries.push(project_entry(
            DiscoveryProject {
                id: "cached".to_owned(),
                slug: "cached".to_owned(),
                title: "Cached".to_owned(),
                description: String::new(),
                downloads: 0,
                icon_url: None,
                icon_bytes: None,
            },
            None,
        ));
        let mut vanilla = instance("vanilla", "1.21.1");
        vanilla.loader = ModLoader::Vanilla;

        state.set_unavailable(&vanilla);

        assert!(state.list.entries.is_empty());
        assert!(!state.page_loading);
        assert!(state.exhausted);
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
                None
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
                None
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
                None
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
                None
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
                None
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
                None
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
                None
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
