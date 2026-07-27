use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent};

use crate::instance::{ContentEntry, ContentKind, InstanceConfig, ModLoader};
use crate::net::modrinth::{DiscoveryProject, VersionInfo};

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
    pub kind: ContentKind,
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
    pub fn new(kind: ContentKind) -> Self {
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

    pub fn project_link_at(&self, x: u16, y: u16) -> Option<&str> {
        self.project_page.as_ref()?.document.as_ref()?.link_at(x, y)
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
            crate::feedback::request_redraw();
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
        crate::feedback::request_redraw();
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
            crate::feedback::request_redraw();
        }
    }

    pub fn push_action_result(pending: &PendingActions, result: DiscoveryActionResult) {
        if let Ok(mut pending) = pending.lock() {
            pending.push(result);
            crate::feedback::request_redraw();
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
                        crate::feedback::errors::push_error(crate::feedback::errors::ErrorEvent {
                            id: request_id,
                            level: tracing::Level::INFO,
                            message: format!("{project_title}: {action}"),
                            pushed_at: std::time::Instant::now(),
                        });
                    }
                    Err(error) => {
                        crate::feedback::errors::push_error(crate::feedback::errors::ErrorEvent {
                            id: request_id,
                            level: tracing::Level::ERROR,
                            message: format!("{project_title}: {error}"),
                            pushed_at: std::time::Instant::now(),
                        });
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
        icon_lines: Some(crate::instance::content::fallback_icon()),
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
#[path = "../../tests/widgets/content/discovery.rs"]
mod tests;
