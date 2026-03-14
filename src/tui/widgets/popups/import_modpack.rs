use super::base::PopupFrame;
use super::new_instance::LoadState;
use crate::instance::import::ImportSummary;
use crate::net::modrinth::{self, ModrinthInput, VersionInfo};
use crate::tui::layout::FocusedArea;
use crate::tui::theme::THEME;
use crate::tui::widgets::profiles;
use crate::tui::widgets::search::SearchState;
use crossterm::event::{KeyCode, KeyEvent};
use once_cell::sync::Lazy;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph, StatefulWidget, Widget, Wrap},
    Frame,
};
use std::sync::{Arc, Mutex};

static IMPORT_STATE: Lazy<Arc<Mutex<ImportWizardState>>> =
    Lazy::new(|| Arc::new(Mutex::new(ImportWizardState::default())));
static IMPORT_RESULT: Lazy<Arc<Mutex<Option<ImportResult>>>> =
    Lazy::new(|| Arc::new(Mutex::new(None)));

#[derive(Debug, Clone)]
pub struct ImportResult {
    pub summary: ImportSummary,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum ImportStep {
    #[default]
    Source,
    Input,
    Fetching,
    Version,
    Confirm,
}

#[derive(Debug, Clone)]
pub struct ImportWizardState {
    pub step: ImportStep,
    pub source_idx: usize,
    pub input: String,
    pub error: Option<String>,
    pub project_title: Option<String>,
    pub versions: LoadState<Vec<VersionInfo>>,
    pub version_idx: usize,
    pub version_search: SearchState,
    pub summary: Option<ImportSummary>,
}

impl Default for ImportWizardState {
    fn default() -> Self {
        Self {
            step: ImportStep::Source,
            source_idx: 0,
            input: String::new(),
            error: None,
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

// --- Public API ---

pub fn render(frame: &mut Frame, area: Rect, _focused: FocusedArea) {
    let snapshot = match IMPORT_STATE.lock() {
        Ok(state) => state.clone(),
        Err(e) => {
            tracing::error!("Import state lock poisoned: {}", e);
            ImportWizardState::default()
        }
    };

    let keybinds = step_keybinds(&snapshot);

    let search_line = if snapshot.step == ImportStep::Version {
        snapshot.version_search.title_line()
    } else {
        None
    };

    let popup = PopupFrame {
        title: wizard_title(&snapshot),
        border_color: THEME.popup_new_instance.border_fg,
        bg: Some(THEME.popup_new_instance.bg),
        keybinds: Some(keybinds),
        search_line,
        content: Box::new(move |popup_area, buf| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1)])
                .split(popup_area);

            match snapshot.step {
                ImportStep::Source => render_source_step(&snapshot, chunks[0], buf),
                ImportStep::Input => render_input_step(&snapshot, chunks[0], buf),
                ImportStep::Fetching => render_fetching_step(chunks[0], buf),
                ImportStep::Version => render_version_step(&snapshot, chunks[0], buf),
                ImportStep::Confirm => render_confirm_step(&snapshot, chunks[0], buf),
            }
        }),
    };

    frame.render_widget(popup, area);
}

pub fn popup_rect(frame_area: Rect) -> Rect {
    let w = Constraint::Percentage(50);
    let step = match IMPORT_STATE.lock() {
        Ok(s) => s.step.clone(),
        Err(_) => ImportStep::Source,
    };

    match step {
        ImportStep::Source => {
            let h = 5u16.min(frame_area.height.saturating_sub(4));
            frame_area.centered(w, Constraint::Length(h))
        }
        ImportStep::Input => {
            let h = 8u16.min(frame_area.height.saturating_sub(4));
            frame_area.centered(w, Constraint::Length(h))
        }
        ImportStep::Fetching => {
            let h = 5u16.min(frame_area.height.saturating_sub(4));
            frame_area.centered(w, Constraint::Length(h))
        }
        ImportStep::Version => {
            let h = (frame_area.height * 2 / 3)
                .max(10)
                .min(frame_area.height.saturating_sub(4));
            frame_area.centered(w, Constraint::Length(h))
        }
        ImportStep::Confirm => {
            let h = 10u16.min(frame_area.height.saturating_sub(4));
            frame_area.centered(w, Constraint::Length(h))
        }
    }
}

pub fn handle_key(key_event: &KeyEvent, profiles_state: &mut profiles::State) {
    let mut state = match IMPORT_STATE.lock() {
        Ok(state) => state,
        Err(e) => {
            tracing::error!("Import state lock poisoned: {}", e);
            profiles_state.show_import_popup = false;
            return;
        }
    };

    match state.step {
        ImportStep::Source => handle_source_key(&mut state, key_event, profiles_state),
        ImportStep::Input => handle_input_key(&mut state, key_event, profiles_state),
        ImportStep::Fetching => handle_fetching_key(&mut state, key_event, profiles_state),
        ImportStep::Version => handle_version_key(&mut state, key_event, profiles_state),
        ImportStep::Confirm => handle_confirm_key(&mut state, key_event, profiles_state),
    }
}

pub fn take_result() -> Option<ImportResult> {
    match IMPORT_RESULT.lock() {
        Ok(mut r) => r.take(),
        Err(_) => None,
    }
}

// --- Step handlers ---

fn handle_source_key(
    state: &mut ImportWizardState,
    key_event: &KeyEvent,
    profiles_state: &mut profiles::State,
) {
    // Only one source for now (Modrinth), so j/k navigate but stay at 0
    match key_event.code {
        KeyCode::Esc => close_popup(state, profiles_state),
        KeyCode::Char('j') | KeyCode::Down => {
            // Only 1 source for now
            state.source_idx = 0;
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.source_idx = 0;
        }
        KeyCode::Enter => {
            state.step = ImportStep::Input;
        }
        _ => {}
    }
}

fn handle_input_key(
    state: &mut ImportWizardState,
    key_event: &KeyEvent,
    profiles_state: &mut profiles::State,
) {
    match key_event.code {
        KeyCode::Esc => close_popup(state, profiles_state),
        KeyCode::Left | KeyCode::Char('h') if state.input.is_empty() => {
            state.step = ImportStep::Source;
            state.error = None;
        }
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
    profiles_state: &mut profiles::State,
) {
    if key_event.code == KeyCode::Esc {
        close_popup(state, profiles_state);
    }
}

fn handle_version_key(
    state: &mut ImportWizardState,
    key_event: &KeyEvent,
    profiles_state: &mut profiles::State,
) {
    // Search mode: route char input to search query
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
            KeyCode::Char('j') | KeyCode::Down => {
                // fall through to navigation below
            }
            KeyCode::Char('k') | KeyCode::Up => {
                // fall through to navigation below
            }
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
        KeyCode::Esc => close_popup(state, profiles_state),
        KeyCode::Left | KeyCode::Char('h') if !state.version_search.active => {
            state.step = ImportStep::Input;
            state.versions = LoadState::Idle;
            state.version_idx = 0;
            state.version_search.deactivate();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if visible_count > 0 {
                state.version_idx = (state.version_idx + 1).min(visible_count.saturating_sub(1));
            }
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
    profiles_state: &mut profiles::State,
) {
    match key_event.code {
        KeyCode::Esc => close_popup(state, profiles_state),
        KeyCode::Left | KeyCode::Char('h') => {
            // Go back -- if we had versions, go to Version; otherwise go to Input
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

            close_popup(state, profiles_state);
        }
        _ => {}
    }
}

// --- Async resolution ---

fn start_resolve(state: &mut ImportWizardState) {
    let input_text = state.input.clone();
    state.step = ImportStep::Fetching;
    state.error = None;
    let state_arc = IMPORT_STATE.clone();

    tokio::spawn(async move {
        let client = crate::net::HttpClient::new();
        let parsed = modrinth::parse_input(&input_text);

        match parsed {
            ModrinthInput::ProjectSlug(slug) => {
                resolve_project_slug(state_arc, &client, &slug).await;
            }
            ModrinthInput::VersionId {
                slug: _,
                version_id,
            } => {
                resolve_version_id(state_arc, &client, &version_id).await;
            }
            ModrinthInput::LocalFile(path) => {
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
            Err(e) => {
                if let Ok(mut s) = state_arc.lock() {
                    s.error = Some(format!("Failed to fetch versions: {}", e));
                    s.step = ImportStep::Input;
                }
            }
        },
        Err(e) => {
            if let Ok(mut s) = state_arc.lock() {
                s.error = Some(format!("Failed to fetch project: {}", e));
                s.step = ImportStep::Input;
            }
        }
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
                if let Ok(mut s) = state_arc.lock() {
                    s.error = Some(format!("Failed to create tmp dir: {}", e));
                    s.step = ImportStep::Input;
                }
                return;
            }

            match modrinth::download_mrpack(client, &version, &tmp_dir).await {
                Ok(mrpack_path) => match modrinth::parse_mrpack(&mrpack_path) {
                    Ok(index) => {
                        match crate::instance::import::build_summary(&index, mrpack_path) {
                            Ok(summary) => {
                                if let Ok(mut s) = state_arc.lock() {
                                    s.summary = Some(summary);
                                    s.step = ImportStep::Confirm;
                                }
                            }
                            Err(e) => {
                                if let Ok(mut s) = state_arc.lock() {
                                    s.error = Some(format!("Failed to build summary: {}", e));
                                    s.step = ImportStep::Input;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        if let Ok(mut s) = state_arc.lock() {
                            s.error = Some(format!("Failed to parse mrpack: {}", e));
                            s.step = ImportStep::Input;
                        }
                    }
                },
                Err(e) => {
                    if let Ok(mut s) = state_arc.lock() {
                        s.error = Some(format!("Failed to download mrpack: {}", e));
                        s.step = ImportStep::Input;
                    }
                }
            }
        }
        Err(e) => {
            if let Ok(mut s) = state_arc.lock() {
                s.error = Some(format!("Failed to fetch version: {}", e));
                s.step = ImportStep::Input;
            }
        }
    }
}

fn resolve_local_file(state_arc: Arc<Mutex<ImportWizardState>>, path: &str) {
    let resolved = if let Some(stripped) = path.strip_prefix("~/") {
        match dirs_next::home_dir() {
            Some(home) => home.join(stripped),
            None => std::path::PathBuf::from(path),
        }
    } else {
        std::path::PathBuf::from(path)
    };

    match modrinth::parse_mrpack(&resolved) {
        Ok(index) => {
            match crate::instance::import::build_summary(&index, resolved) {
                Ok(summary) => {
                    if let Ok(mut s) = state_arc.lock() {
                        s.summary = Some(summary);
                        s.step = ImportStep::Confirm;
                    }
                }
                Err(e) => {
                    if let Ok(mut s) = state_arc.lock() {
                        s.error = Some(format!("Failed to build summary: {}", e));
                        s.step = ImportStep::Input;
                    }
                }
            }
        }
        Err(e) => {
            if let Ok(mut s) = state_arc.lock() {
                s.error = Some(format!("Failed to parse mrpack: {}", e));
                s.step = ImportStep::Input;
            }
        }
    }
}

fn start_version_download(state: &mut ImportWizardState) {
    let version = match selected_version(state) {
        Some(v) => v.clone(),
        None => return,
    };

    state.step = ImportStep::Fetching;
    state.error = None;
    let state_arc = IMPORT_STATE.clone();

    tokio::spawn(async move {
        let client = crate::net::HttpClient::new();
        let meta_dir = crate::config::SETTINGS.paths.resolve_meta_dir();
        let tmp_dir = meta_dir.join("tmp");
        if let Err(e) = tokio::fs::create_dir_all(&tmp_dir).await {
            if let Ok(mut s) = state_arc.lock() {
                s.error = Some(format!("Failed to create tmp dir: {}", e));
                s.step = ImportStep::Version;
            }
            return;
        }

        match modrinth::download_mrpack(&client, &version, &tmp_dir).await {
            Ok(mrpack_path) => match modrinth::parse_mrpack(&mrpack_path) {
                Ok(index) => {
                    match crate::instance::import::build_summary(&index, mrpack_path) {
                        Ok(summary) => {
                            if let Ok(mut s) = state_arc.lock() {
                                s.summary = Some(summary);
                                s.step = ImportStep::Confirm;
                            }
                        }
                        Err(e) => {
                            if let Ok(mut s) = state_arc.lock() {
                                s.error = Some(format!("Failed to build summary: {}", e));
                                s.step = ImportStep::Version;
                            }
                        }
                    }
                }
                Err(e) => {
                    if let Ok(mut s) = state_arc.lock() {
                        s.error = Some(format!("Failed to parse mrpack: {}", e));
                        s.step = ImportStep::Version;
                    }
                }
            },
            Err(e) => {
                if let Ok(mut s) = state_arc.lock() {
                    s.error = Some(format!("Failed to download mrpack: {}", e));
                    s.step = ImportStep::Version;
                }
            }
        }
    });
}

// --- Render helpers ---

fn render_source_step(
    state: &ImportWizardState,
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    let items: Vec<ListItem> = vec![ListItem::new(Line::from(Span::styled(
        "Modrinth",
        Style::default().fg(THEME.popup_new_instance.text_fg),
    )))];

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(THEME.popup_new_instance.accent_fg)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("\u{25b6} ");

    let mut list_state = ListState::default().with_selected(Some(state.source_idx));
    StatefulWidget::render(list, area, buf, &mut list_state);
}

fn render_input_step(
    state: &ImportWizardState,
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // input line
            Constraint::Length(1), // error or hint
            Constraint::Min(0),   // remaining space
        ])
        .split(area);

    // Input line with blinking cursor
    let input_line = if state.input.is_empty() {
        Line::from(vec![
            Span::styled(
                "URL, slug, or .mrpack path...",
                Style::default().fg(THEME.popup_new_instance.field_inactive_border_fg),
            ),
            Span::styled(
                "\u{2588}",
                Style::default()
                    .fg(THEME.popup_new_instance.border_fg)
                    .add_modifier(Modifier::SLOW_BLINK),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                state.input.clone(),
                Style::default().fg(THEME.popup_new_instance.text_fg),
            ),
            Span::styled(
                "\u{2588}",
                Style::default()
                    .fg(THEME.popup_new_instance.border_fg)
                    .add_modifier(Modifier::SLOW_BLINK),
            ),
        ])
    };
    Paragraph::new(input_line).render(chunks[0], buf);

    // Error or hint
    if let Some(ref err) = state.error {
        Paragraph::new(Span::styled(
            err.as_str(),
            Style::default().fg(THEME.popup_new_instance.error_fg),
        ))
        .wrap(Wrap { trim: true })
        .render(chunks[1], buf);
    } else {
        Paragraph::new(Span::styled(
            "URL, slug, or local .mrpack path",
            Style::default().fg(THEME.popup_new_instance.field_inactive_border_fg),
        ))
        .render(chunks[1], buf);
    }
}

fn render_fetching_step(area: Rect, buf: &mut ratatui::buffer::Buffer) {
    Paragraph::new("Fetching modpack info...")
        .style(Style::default().fg(THEME.popup_new_instance.field_inactive_border_fg))
        .render(area, buf);
}

fn render_version_step(
    state: &ImportWizardState,
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    match &state.versions {
        LoadState::Idle | LoadState::Loading => {
            Paragraph::new("Loading versions...")
                .style(Style::default().fg(THEME.popup_new_instance.field_inactive_border_fg))
                .render(area, buf);
        }
        LoadState::Error(message) => {
            Paragraph::new(message.as_str())
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(THEME.popup_new_instance.error_fg))
                .render(area, buf);
        }
        LoadState::Loaded(_) => {
            let items: Vec<ListItem> = visible_versions(state)
                .into_iter()
                .map(|version| {
                    let game_ver = version.game_versions.first().cloned().unwrap_or_default();
                    let loader = version.loaders.first().cloned().unwrap_or_default();
                    ListItem::new(Line::from(Span::styled(
                        format!("{}  {}  {}", version.version_number, game_ver, loader),
                        Style::default().fg(THEME.popup_new_instance.text_fg),
                    )))
                })
                .collect();

            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .fg(THEME.popup_new_instance.accent_fg)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("\u{25b6} ");

            let mut list_state = ListState::default().with_selected(Some(state.version_idx));
            StatefulWidget::render(list, area, buf, &mut list_state);
        }
    }
}

fn render_confirm_step(
    state: &ImportWizardState,
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    let summary = match &state.summary {
        Some(s) => s,
        None => {
            Paragraph::new("No summary available")
                .style(Style::default().fg(THEME.popup_new_instance.field_inactive_border_fg))
                .render(area, buf);
            return;
        }
    };

    let label_style = Style::default().fg(THEME.popup_new_instance.border_fg);

    let loader_display = if let Some(ref lv) = summary.loader_version {
        format!("{} {}", summary.loader, lv)
    } else {
        summary.loader.to_string()
    };

    Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Name: ", label_style),
            Span::raw(summary.name.clone()),
        ]),
        Line::from(vec![
            Span::styled("Pack Version: ", label_style),
            Span::raw(summary.pack_version.clone()),
        ]),
        Line::from(vec![
            Span::styled("MC Version: ", label_style),
            Span::raw(summary.game_version.clone()),
        ]),
        Line::from(vec![
            Span::styled("Loader: ", label_style),
            Span::raw(loader_display),
        ]),
        Line::from(vec![
            Span::styled("Mods: ", label_style),
            Span::raw(format!("{} files", summary.mod_count)),
        ]),
        Line::from(vec![
            Span::styled("Overrides: ", label_style),
            Span::raw(format!("{} files", summary.override_count)),
        ]),
    ])
    .style(Style::default().fg(THEME.popup_new_instance.text_fg))
    .wrap(Wrap { trim: true })
    .render(area, buf);
}

// --- Helpers ---

fn close_popup(state: &mut ImportWizardState, profiles_state: &mut profiles::State) {
    state.reset();
    profiles_state.show_import_popup = false;
}

fn wizard_title(_state: &ImportWizardState) -> Line<'static> {
    use crate::tui::widgets::styled_title;
    styled_title("Import Modpack", false)
}

fn step_keybinds(state: &ImportWizardState) -> Line<'static> {
    use super::keybind_line;
    match state.step {
        ImportStep::Source => keybind_line(&[("Enter", " select")]),
        ImportStep::Input => keybind_line(&[("h", " back"), ("Enter", " submit")]),
        ImportStep::Fetching => keybind_line(&[("Esc", " cancel")]),
        ImportStep::Version => keybind_line(&[
            ("/", " search"),
            ("h", " back"),
            ("Enter", " select"),
        ]),
        ImportStep::Confirm => keybind_line(&[("h", " back"), ("Enter", " import")]),
    }
}

fn selected_version(state: &ImportWizardState) -> Option<&VersionInfo> {
    if let LoadState::Loaded(ref versions) = state.versions {
        let visible: Vec<_> = visible_versions_ref(versions, &state.version_search);
        visible.get(state.version_idx).copied()
    } else {
        None
    }
}

fn visible_versions(state: &ImportWizardState) -> Vec<VersionInfo> {
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

fn clamp_version_index(state: &mut ImportWizardState) {
    let count = visible_versions(state).len();
    if count == 0 {
        state.version_idx = 0;
    } else if state.version_idx >= count {
        state.version_idx = count.saturating_sub(1);
    }
}
