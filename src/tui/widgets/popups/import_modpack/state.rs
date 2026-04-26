// state machine for the modpack import wizard.
// accepts modrinth URLs, project slugs, version IDs, or local pack archives
// (.mrpack, mmc/prism zips). remote packs go through version selection,
// local files skip straight to the confirm step.

use super::super::new_instance::LoadState;
use crate::instance::import::{ImportInput, ImportSummary, parse_import_input};
use crate::net::modrinth::{self, VersionInfo};
use crate::tui::widgets::instances;
use crate::tui::widgets::search::SearchState;
use crossterm::event::{KeyCode, KeyEvent};
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::Level;

pub(super) static IMPORT_STATE: LazyLock<Arc<Mutex<ImportWizardState>>> =
    LazyLock::new(|| Arc::new(Mutex::new(ImportWizardState::default())));
pub(super) static IMPORT_RESULT: LazyLock<Arc<Mutex<Option<ImportResult>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(None)));

#[derive(Debug, Clone)]
pub struct ImportResult {
    pub summary: ImportSummary,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum ImportStep {
    #[default]
    Input,
    Fetching,
    Version,
    Confirm,
}

#[derive(Debug, Clone)]
pub struct ImportWizardState {
    pub step: ImportStep,
    pub input: String,
    pub project_title: Option<String>,
    pub versions: LoadState<Vec<VersionInfo>>,
    pub version_idx: usize,
    pub version_search: SearchState,
    pub summary: Option<ImportSummary>,
}

impl Default for ImportWizardState {
    fn default() -> Self {
        Self {
            step: ImportStep::Input,
            input: String::new(),
            project_title: None,
            versions: LoadState::Idle,
            version_idx: 0,
            version_search: SearchState::default(),
            summary: None,
        }
    }
}

impl ImportWizardState {
    pub fn reset(&mut self) {
        *self = ImportWizardState::default();
    }
}

pub fn handle_key(key_event: &KeyEvent, instances_state: &mut instances::State) {
    let mut state = match IMPORT_STATE.lock() {
        Ok(state) => state,
        Err(e) => {
            tracing::error!("Import state lock poisoned: {}", e);
            instances_state.show_import_popup = false;
            return;
        }
    };

    match state.step {
        ImportStep::Input => handle_input_key(&mut state, key_event, instances_state),
        ImportStep::Fetching => handle_fetching_key(&mut state, key_event, instances_state),
        ImportStep::Version => handle_version_key(&mut state, key_event, instances_state),
        ImportStep::Confirm => handle_confirm_key(&mut state, key_event, instances_state),
    }
}

pub fn take_result() -> Option<ImportResult> {
    match IMPORT_RESULT.lock() {
        Ok(mut r) => r.take(),
        Err(_) => None,
    }
}

fn handle_input_key(
    state: &mut ImportWizardState,
    key_event: &KeyEvent,
    instances_state: &mut instances::State,
) {
    match key_event.code {
        KeyCode::Esc => close_popup(state, instances_state),
        KeyCode::Backspace => {
            state.input.pop();
        }
        KeyCode::Enter => {
            if state.input.trim().is_empty() {
                return;
            }
            start_resolve(state);
        }
        KeyCode::Char(c) => {
            state.input.push(c);
        }
        _ => {}
    }
}

fn handle_fetching_key(
    state: &mut ImportWizardState,
    key_event: &KeyEvent,
    instances_state: &mut instances::State,
) {
    if key_event.code == KeyCode::Esc {
        close_popup(state, instances_state);
    }
}

fn handle_version_key(
    state: &mut ImportWizardState,
    key_event: &KeyEvent,
    instances_state: &mut instances::State,
) {
    if state.version_search.active {
        match key_event.code {
            KeyCode::Esc => {
                state.version_search.deactivate();
                clamp_version_index(state);
                return;
            }
            KeyCode::Backspace => {
                state.version_search.pop();
                clamp_version_index(state);
                return;
            }
            KeyCode::Char('j') | KeyCode::Down => {}
            KeyCode::Char('k') | KeyCode::Up => {}
            KeyCode::Enter => {
                state.version_search.active = false;
                return;
            }
            KeyCode::Char(c) => {
                state.version_search.push(c);
                state.version_idx = 0;
                return;
            }
            _ => {}
        }
    }

    let visible_count = visible_versions(state).len();

    match key_event.code {
        KeyCode::Esc => close_popup(state, instances_state),
        KeyCode::Left | KeyCode::Char('h') if !state.version_search.active => {
            state.step = ImportStep::Input;
            state.versions = LoadState::Idle;
            state.version_idx = 0;
            state.version_search.deactivate();
        }
        KeyCode::Char('j') | KeyCode::Down if visible_count > 0 => {
            state.version_idx = (state.version_idx + 1).min(visible_count.saturating_sub(1));
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.version_idx = state.version_idx.saturating_sub(1);
        }
        KeyCode::Char('/') if !state.version_search.active => {
            state.version_search.activate();
            state.version_idx = 0;
        }
        KeyCode::Enter if !state.version_search.active => {
            let selected = selected_version(state);
            if selected.is_none() {
                return;
            }
            start_version_download(state);
        }
        _ => {}
    }
}

fn handle_confirm_key(
    state: &mut ImportWizardState,
    key_event: &KeyEvent,
    instances_state: &mut instances::State,
) {
    match key_event.code {
        KeyCode::Esc => close_popup(state, instances_state),
        // if it came from a local file, there's no version list to go back to
        KeyCode::Left | KeyCode::Char('h') => {
            if matches!(state.versions, LoadState::Loaded(_)) {
                state.step = ImportStep::Version;
            } else {
                state.step = ImportStep::Input;
            }
        }
        KeyCode::Enter => {
            let summary = match state.summary.take() {
                Some(s) => s,
                None => return,
            };

            match IMPORT_RESULT.lock() {
                Ok(mut result) => {
                    *result = Some(ImportResult { summary });
                }
                Err(e) => {
                    tracing::error!("Import result lock poisoned: {}", e);
                }
            }

            close_popup(state, instances_state);
        }
        _ => {}
    }
}

// pushes an error toast and rewinds the wizard to a previous step
fn set_error_and_back(state_arc: &Arc<Mutex<ImportWizardState>>, msg: String, step: ImportStep) {
    push_import_error(msg);
    if let Ok(mut s) = state_arc.lock() {
        s.step = step;
    }
}

// parses user input to figure out what they gave us, then dispatches
// to the appropriate resolve path (slug lookup, direct version, or local file)
fn start_resolve(state: &mut ImportWizardState) {
    let input_text = state.input.clone();
    state.step = ImportStep::Fetching;

    let state_arc = IMPORT_STATE.clone();

    tokio::spawn(async move {
        let client = crate::net::HttpClient::new();
        let parsed = parse_import_input(&input_text);

        match parsed {
            ImportInput::ProjectSlug(slug) => {
                resolve_project_slug(state_arc, &client, &slug).await;
            }
            ImportInput::VersionId {
                slug: _,
                version_id,
            } => {
                resolve_version_id(state_arc, &client, &version_id).await;
            }
            ImportInput::LocalFile(path) => {
                resolve_local_file(state_arc, &path);
            }
        }
    });
}

async fn resolve_project_slug(
    state_arc: Arc<Mutex<ImportWizardState>>,
    client: &crate::net::HttpClient,
    slug: &str,
) {
    match modrinth::fetch_project(client, slug).await {
        Ok(project) => match modrinth::fetch_versions(client, slug).await {
            Ok(versions) => {
                if let Ok(mut s) = state_arc.lock() {
                    s.project_title = Some(project.title);
                    s.versions = LoadState::Loaded(versions);
                    s.version_idx = 0;
                    s.version_search.deactivate();
                    s.step = ImportStep::Version;
                }
            }
            Err(e) => set_error_and_back(
                &state_arc,
                format!("Failed to fetch versions: {}", e),
                ImportStep::Input,
            ),
        },
        Err(e) => set_error_and_back(
            &state_arc,
            format!("Failed to fetch project: {}", e),
            ImportStep::Input,
        ),
    }
}

async fn resolve_version_id(
    state_arc: Arc<Mutex<ImportWizardState>>,
    client: &crate::net::HttpClient,
    version_id: &str,
) {
    match modrinth::fetch_version(client, version_id).await {
        Ok(version) => {
            let meta_dir = crate::config::SETTINGS.paths.resolve_meta_dir();
            let tmp_dir = meta_dir.join("tmp");
            if let Err(e) = tokio::fs::create_dir_all(&tmp_dir).await {
                set_error_and_back(
                    &state_arc,
                    format!("Failed to create tmp dir: {}", e),
                    ImportStep::Input,
                );
                return;
            }

            match modrinth::download_mrpack(client, &version, &tmp_dir).await {
                Ok(mrpack_path) => match crate::instance::import::build_summary(&mrpack_path) {
                    Ok(summary) => {
                        if let Ok(mut s) = state_arc.lock() {
                            s.summary = Some(summary);
                            s.step = ImportStep::Confirm;
                        }
                    }
                    Err(e) => set_error_and_back(
                        &state_arc,
                        format!("Failed to build summary: {}", e),
                        ImportStep::Input,
                    ),
                },
                Err(e) => set_error_and_back(
                    &state_arc,
                    format!("Failed to download mrpack: {}", e),
                    ImportStep::Input,
                ),
            }
        }
        Err(e) => set_error_and_back(
            &state_arc,
            format!("Failed to fetch version: {}", e),
            ImportStep::Input,
        ),
    }
}

fn resolve_local_file(state_arc: Arc<Mutex<ImportWizardState>>, path: &str) {
    let resolved = crate::config::settings::resolve_path(path);

    match crate::instance::import::build_summary(&resolved) {
        Ok(summary) => {
            if let Ok(mut s) = state_arc.lock() {
                s.summary = Some(summary);
                s.step = ImportStep::Confirm;
            }
        }
        Err(e) => set_error_and_back(
            &state_arc,
            format!("Failed to parse pack: {}", e),
            ImportStep::Input,
        ),
    }
}

// user picked a version from the list. download the .mrpack,
// build a summary, and move to confirm.
fn start_version_download(state: &mut ImportWizardState) {
    let version = match selected_version(state) {
        Some(v) => v.clone(),
        None => return,
    };

    state.step = ImportStep::Fetching;

    let state_arc = IMPORT_STATE.clone();

    tokio::spawn(async move {
        let client = crate::net::HttpClient::new();
        let meta_dir = crate::config::SETTINGS.paths.resolve_meta_dir();
        let tmp_dir = meta_dir.join("tmp");
        if let Err(e) = tokio::fs::create_dir_all(&tmp_dir).await {
            set_error_and_back(
                &state_arc,
                format!("Failed to create tmp dir: {}", e),
                ImportStep::Version,
            );
            return;
        }

        match modrinth::download_mrpack(&client, &version, &tmp_dir).await {
            Ok(mrpack_path) => match crate::instance::import::build_summary(&mrpack_path) {
                Ok(summary) => {
                    if let Ok(mut s) = state_arc.lock() {
                        s.summary = Some(summary);
                        s.step = ImportStep::Confirm;
                    }
                }
                Err(e) => set_error_and_back(
                    &state_arc,
                    format!("Failed to build summary: {}", e),
                    ImportStep::Version,
                ),
            },
            Err(e) => set_error_and_back(
                &state_arc,
                format!("Failed to download mrpack: {}", e),
                ImportStep::Version,
            ),
        }
    });
}

fn close_popup(state: &mut ImportWizardState, instances_state: &mut instances::State) {
    state.reset();
    instances_state.show_import_popup = false;
}

fn push_import_error(msg: String) {
    crate::tui::error_buffer::push_error(crate::tui::error_buffer::ErrorEvent {
        id: 0,
        level: Level::ERROR,
        message: msg,
        pushed_at: Instant::now(),
    });
}

pub(super) fn visible_versions(state: &ImportWizardState) -> Vec<VersionInfo> {
    match &state.versions {
        LoadState::Loaded(versions) => {
            let q = state.version_search.query.to_lowercase();
            versions
                .iter()
                .filter(|v| {
                    q.is_empty()
                        || v.name.to_lowercase().contains(&q)
                        || v.version_number.to_lowercase().contains(&q)
                })
                .cloned()
                .collect()
        }
        _ => Vec::new(),
    }
}

fn visible_versions_ref<'a>(
    versions: &'a [VersionInfo],
    search: &SearchState,
) -> Vec<&'a VersionInfo> {
    let q = search.query.to_lowercase();
    versions
        .iter()
        .filter(|v| {
            q.is_empty()
                || v.name.to_lowercase().contains(&q)
                || v.version_number.to_lowercase().contains(&q)
        })
        .collect()
}

fn selected_version(state: &ImportWizardState) -> Option<&VersionInfo> {
    if let LoadState::Loaded(ref versions) = state.versions {
        let visible: Vec<_> = visible_versions_ref(versions, &state.version_search);
        visible.get(state.version_idx).copied()
    } else {
        None
    }
}

fn clamp_version_index(state: &mut ImportWizardState) {
    let count = visible_versions(state).len();
    if count == 0 {
        state.version_idx = 0;
    } else if state.version_idx >= count {
        state.version_idx = count.saturating_sub(1);
    }
}
