use super::base::PopupFrame;
use crate::instance::{loader::{get_installer, GameVersion}, models::ModLoader};
use crate::tui::layout::FocusedArea;
use crate::tui::theme::THEME;
use crate::tui::widgets::profiles;
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

static WIZARD_STATE: Lazy<Arc<Mutex<WizardState>>> = Lazy::new(|| Arc::new(Mutex::new(WizardState::default())));
static WIZARD_RESULT: Lazy<Arc<Mutex<Option<WizardParams>>>> = Lazy::new(|| Arc::new(Mutex::new(None)));

#[derive(Debug, Clone)]
pub struct WizardParams {
    pub name: String,
    pub game_version: String,
    pub loader: ModLoader,
    pub loader_version: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub enum WizardStep {
    #[default]
    Name,
    Version,
    Loader,
    LoaderVersion,
    Confirm,
}

#[derive(Debug, Clone)]
pub enum LoadState<T> {
    Idle,
    Loading,
    Loaded(T),
    Error(String),
}

impl<T> Default for LoadState<T> {
    fn default() -> Self {
        LoadState::Idle
    }
}

#[derive(Debug, Default, Clone)]
pub struct WizardState {
    pub step: WizardStep,
    pub name_input: String,
    pub versions: LoadState<Vec<GameVersion>>,
    pub version_idx: usize,
    pub show_snapshots: bool,
    pub loader_idx: usize,
    pub loader_versions: LoadState<Vec<String>>,
    pub loader_version_idx: usize,
    pub version_search: crate::tui::widgets::search::SearchState,
}

impl WizardState {
    pub fn reset(&mut self) {
        *self = WizardState::default();
    }

    pub fn selected_version(&self) -> Option<&GameVersion> {
        if let LoadState::Loaded(ref versions) = self.versions {
            let visible: Vec<_> = versions
                .iter()
                .filter(|v| self.show_snapshots || v.stable)
                .collect();
            visible.get(self.version_idx).copied()
        } else {
            None
        }
    }

    pub fn selected_loader(&self) -> ModLoader {
        const LOADERS: [ModLoader; 5] = [
            ModLoader::Vanilla,
            ModLoader::Fabric,
            ModLoader::Forge,
            ModLoader::NeoForge,
            ModLoader::Quilt,
        ];
        LOADERS[self.loader_idx % 5]
    }

    pub fn selected_loader_version(&self) -> Option<String> {
        if let LoadState::Loaded(ref versions) = self.loader_versions {
            versions.get(self.loader_version_idx).cloned()
        } else {
            None
        }
    }
}

pub fn render(frame: &mut Frame, area: Rect, _focused: FocusedArea) {
    let snapshot = match WIZARD_STATE.lock() {
        Ok(mut state) => {
            if state.step == WizardStep::Version {
                ensure_versions_loaded(&mut state);
                clamp_version_index(&mut state);
            }

            if state.step == WizardStep::LoaderVersion {
                if state.selected_loader() == ModLoader::Vanilla {
                    state.step = WizardStep::Confirm;
                } else {
                    clamp_loader_version_index(&mut state);
                    let game_version = state.selected_version().map(|v| v.id.clone());
                    let loader = state.selected_loader();
                    if let Some(game_version) = game_version {
                        ensure_loader_versions_loaded(&mut state, loader, game_version);
                    }
                }
            }

            state.clone()
        }
        Err(e) => {
            tracing::error!("Wizard state lock poisoned: {}", e);
            WizardState::default()
        }
    };

    let keybinds = step_keybinds(&snapshot);

    let search_line = snapshot.version_search.title_line();

    let popup = PopupFrame {
        title: wizard_title(&snapshot),
        border_color: THEME.colors.border_focused,
        bg: Some(THEME.colors.row_alternate_bg),
        keybinds: Some(keybinds),
        search_line,
        content: Box::new(move |popup_area, buf| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1)])
                .split(popup_area);

            match snapshot.step {
                WizardStep::Name => render_name_step(&snapshot, chunks[0], buf),
                WizardStep::Version => render_version_step(&snapshot, chunks[0], buf),
                WizardStep::Loader => render_loader_step(&snapshot, chunks[0], buf),
                WizardStep::LoaderVersion => render_loader_version_step(&snapshot, chunks[0], buf),
                WizardStep::Confirm => render_confirm_step(&snapshot, chunks[0], buf),
            }
        }),
    };

    frame.render_widget(popup, area);
}

pub fn handle_key(key_event: &KeyEvent, profiles_state: &mut profiles::State) {
    let mut state = match WIZARD_STATE.lock() {
        Ok(state) => state,
        Err(e) => {
            tracing::error!("Wizard state lock poisoned: {}", e);
            profiles_state.show_popup = false;
            return;
        }
    };

    match state.step {
        WizardStep::Name => handle_name_key(&mut state, key_event, profiles_state),
        WizardStep::Version => handle_version_key(&mut state, key_event, profiles_state),
        WizardStep::Loader => handle_loader_key(&mut state, key_event, profiles_state),
        WizardStep::LoaderVersion => handle_loader_version_key(&mut state, key_event, profiles_state),
        WizardStep::Confirm => handle_confirm_key(&mut state, key_event, profiles_state),
    }
}

pub fn take_result() -> Option<WizardParams> {
    match WIZARD_RESULT.lock() {
        Ok(mut r) => r.take(),
        Err(_) => None,
    }
}

pub fn popup_rect(frame_area: Rect) -> Rect {
    use ratatui::layout::Constraint;

    let step = match WIZARD_STATE.lock() {
        Ok(s) => s.step.clone(),
        Err(_) => WizardStep::Name,
    };

    let w = Constraint::Percentage(50);

    match step {
        WizardStep::Name => {
            let h = 6u16.min(frame_area.height.saturating_sub(4));
            frame_area.centered(w, Constraint::Length(h))
        }
        WizardStep::Version | WizardStep::LoaderVersion => {
            let h = (frame_area.height * 2 / 3).max(10).min(frame_area.height.saturating_sub(4));
            frame_area.centered(w, Constraint::Length(h))
        }
        WizardStep::Loader => {
            let h = 9u16.min(frame_area.height.saturating_sub(4));
            frame_area.centered(w, Constraint::Length(h))
        }
        WizardStep::Confirm => {
            let h = 8u16.min(frame_area.height.saturating_sub(4));
            frame_area.centered(w, Constraint::Length(h))
        }
    }
}

fn handle_name_key(
    state: &mut WizardState,
    key_event: &KeyEvent,
    profiles_state: &mut profiles::State,
) {
    match key_event.code {
        KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
            close_popup(state, profiles_state);
        }
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
            if state.name_input.trim().is_empty() {
                return;
            }
            state.step = WizardStep::Loader;
        }
        KeyCode::Backspace => {
            state.name_input.pop();
        }
        KeyCode::Char(c) => {
            state.name_input.push(c);
        }
        _ => {}
    }
}

fn handle_version_key(
    state: &mut WizardState,
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
            KeyCode::Char(c) => {
                state.version_search.push(c);
                state.version_idx = 0; // reset to top of filtered list
                return;
            }
            _ => {}
        }
    }

    let visible_count = visible_versions(state).len();

    match key_event.code {
        KeyCode::Esc => {
            close_popup(state, profiles_state);
        }
        KeyCode::Left | KeyCode::Char('h') if !state.version_search.active => {
            state.step = WizardStep::Loader;
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if visible_count > 0 {
                state.version_idx = (state.version_idx + 1).min(visible_count.saturating_sub(1));
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.version_idx = state.version_idx.saturating_sub(1);
        }
        KeyCode::Char('s') if !state.version_search.active => {
            state.show_snapshots = !state.show_snapshots;
            clamp_version_index(state);
        }
        KeyCode::Char('/') if !state.version_search.active => {
            state.version_search.activate();
            state.version_idx = 0;
        }
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') if !state.version_search.active => {
            if state.selected_version().is_none() {
                return;
            }
            state.loader_versions = LoadState::Idle;
            state.loader_version_idx = 0;
            if state.selected_loader() == ModLoader::Vanilla {
                state.step = WizardStep::Confirm;
            } else {
                state.step = WizardStep::LoaderVersion;
                let game_version = state.selected_version().map(|v| v.id.clone());
                let loader = state.selected_loader();
                if let Some(gv) = game_version {
                    ensure_loader_versions_loaded(state, loader, gv);
                }
            }
        }
        KeyCode::Enter if state.version_search.active => {
            state.version_search.active = false;
        }
        _ => {}
    }
}

fn handle_loader_key(
    state: &mut WizardState,
    key_event: &KeyEvent,
    profiles_state: &mut profiles::State,
) {
    match key_event.code {
        KeyCode::Esc => close_popup(state, profiles_state),
        KeyCode::Left | KeyCode::Char('h') => state.step = WizardStep::Name,
        KeyCode::Char('j') | KeyCode::Down => {
            state.loader_idx = (state.loader_idx + 1).min(4);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.loader_idx = state.loader_idx.saturating_sub(1);
        }
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
            state.versions = LoadState::Idle;
            state.version_idx = 0;
            state.version_search.deactivate();
            state.version_search.active = false;
            state.step = WizardStep::Version;
            ensure_versions_loaded(state);
        }
        _ => {}
    }
}

fn handle_loader_version_key(
    state: &mut WizardState,
    key_event: &KeyEvent,
    profiles_state: &mut profiles::State,
) {
    if state.selected_loader() == ModLoader::Vanilla {
        state.step = WizardStep::Confirm;
        return;
    }

    let version_count = match &state.loader_versions {
        LoadState::Loaded(versions) => versions.len(),
        _ => 0,
    };

    match key_event.code {
        KeyCode::Esc => close_popup(state, profiles_state),
        KeyCode::Left | KeyCode::Char('h') => state.step = WizardStep::Version,
        KeyCode::Char('j') | KeyCode::Down => {
            if version_count > 0 {
                state.loader_version_idx =
                    (state.loader_version_idx + 1).min(version_count.saturating_sub(1));
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.loader_version_idx = state.loader_version_idx.saturating_sub(1);
        }
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
            if state.selected_loader_version().is_none() {
                return;
            }
            state.step = WizardStep::Confirm;
        }
        _ => {}
    }
}

fn handle_confirm_key(
    state: &mut WizardState,
    key_event: &KeyEvent,
    profiles_state: &mut profiles::State,
) {
    match key_event.code {
        KeyCode::Esc => close_popup(state, profiles_state),
        KeyCode::Left | KeyCode::Char('h') => {
            if state.selected_loader() == ModLoader::Vanilla {
                state.step = WizardStep::Version;
            } else {
                state.step = WizardStep::LoaderVersion;
            }
        }
        KeyCode::Enter => {
            let selected_version = match state.selected_version() {
                Some(version) => version.id.clone(),
                None => return,
            };

            let params = WizardParams {
                name: state.name_input.trim().to_string(),
                game_version: selected_version,
                loader: state.selected_loader(),
                loader_version: if state.selected_loader() == ModLoader::Vanilla {
                    None
                } else {
                    state.selected_loader_version()
                },
            };

            match WIZARD_RESULT.lock() {
                Ok(mut result) => {
                    *result = Some(params);
                }
                Err(e) => {
                    tracing::error!("Wizard result lock poisoned: {}", e);
                }
            }

            close_popup(state, profiles_state);
        }
        _ => {}
    }
}

fn close_popup(state: &mut WizardState, profiles_state: &mut profiles::State) {
    state.reset();
    profiles_state.show_popup = false;
}

fn wizard_title(_state: &WizardState) -> Line<'static> {
    Line::styled(
        "New Instance",
        Style::default()
            .fg(THEME.colors.foreground)
            .add_modifier(Modifier::BOLD),
    )
}

fn step_keybinds(state: &WizardState) -> ratatui::text::Line<'static> {
    use super::keybind_line;
    match state.step {
        WizardStep::Name          => keybind_line(&[("Enter", " continue")]),
        WizardStep::Loader        => keybind_line(&[("h", " back"), ("Enter", " select")]),
        WizardStep::Version       => keybind_line(&[("/", " search"), ("s", " snap"), ("h", " back"), ("Enter", " select")]),
        WizardStep::LoaderVersion => keybind_line(&[("h", " back"), ("Enter", " select")]),
        WizardStep::Confirm       => keybind_line(&[("h", " back"), ("Enter", " create")]),
    }
}

fn render_name_step(state: &WizardState, area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let display = format!("{}█", state.name_input);
    Paragraph::new(display.as_str())
        .style(Style::default().fg(THEME.colors.foreground))
        .wrap(Wrap { trim: false })
        .render(area, buf);
}

fn render_version_step(state: &WizardState, area: Rect, buf: &mut ratatui::buffer::Buffer) {
    match &state.versions {
        LoadState::Idle | LoadState::Loading => {
            Paragraph::new("Loading versions...")
                .style(Style::default().fg(THEME.colors.border_unfocused))
                .render(area, buf);
        }
        LoadState::Error(message) => {
            Paragraph::new(message.as_str())
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(ratatui::style::Color::Red))
                .render(area, buf);
        }
        LoadState::Loaded(_) => {
            let items: Vec<ListItem> = visible_versions(state)
                .into_iter()
                .map(|version| {
                    let suffix = if version.stable {
                        String::new()
                    } else {
                        " (snapshot)".to_string()
                    };
                    ListItem::new(format!("{}{}", version.id, suffix))
                })
                .collect();

            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .fg(THEME.colors.row_highlight)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");

            let mut list_state = ListState::default().with_selected(Some(state.version_idx));
            StatefulWidget::render(list, area, buf, &mut list_state);
        }
    }
}

fn render_loader_step(state: &WizardState, area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let loaders = [
        ModLoader::Vanilla,
        ModLoader::Fabric,
        ModLoader::Forge,
        ModLoader::NeoForge,
        ModLoader::Quilt,
    ];

    let items: Vec<ListItem> = loaders
        .into_iter()
        .map(|loader| ListItem::new(loader.to_string()))
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(THEME.colors.row_highlight)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▶ ");

    let mut list_state = ListState::default().with_selected(Some(state.loader_idx));
    StatefulWidget::render(list, area, buf, &mut list_state);
}

fn render_loader_version_step(
    state: &WizardState,
    area: Rect,
    buf: &mut ratatui::buffer::Buffer,
) {
    if state.selected_loader() == ModLoader::Vanilla {
        Paragraph::new("Vanilla has no loader version.")
            .style(Style::default().fg(THEME.colors.border_unfocused))
            .render(area, buf);
        return;
    }

    match &state.loader_versions {
        LoadState::Idle | LoadState::Loading => {
            Paragraph::new(format!("Loading {} versions...", state.selected_loader()))
                .style(Style::default().fg(THEME.colors.border_unfocused))
                .render(area, buf);
        }
        LoadState::Error(message) => {
            Paragraph::new(message.as_str())
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(ratatui::style::Color::Red))
                .render(area, buf);
        }
        LoadState::Loaded(versions) => {
            let items: Vec<ListItem> = versions
                .iter()
                .map(|version| ListItem::new(version.clone()))
                .collect();

            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .fg(THEME.colors.row_highlight)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("▶ ");

            let mut list_state = ListState::default().with_selected(Some(state.loader_version_idx));
            StatefulWidget::render(list, area, buf, &mut list_state);
        }
    }
}

fn render_confirm_step(state: &WizardState, area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let game_version = state
        .selected_version()
        .map(|version| version.id.as_str())
        .unwrap_or("<not selected>");
    let loader = state.selected_loader();
    let loader_version = if loader == ModLoader::Vanilla {
        "n/a".to_string()
    } else {
        state
            .selected_loader_version()
            .unwrap_or_else(|| "<not selected>".to_string())
    };

    let label_style = Style::default().fg(THEME.colors.border_focused);

    Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Name: ", label_style),
            Span::raw(state.name_input.as_str()),
        ]),
        Line::from(vec![
            Span::styled("MC: ", label_style),
            Span::raw(game_version),
        ]),
        Line::from(vec![
            Span::styled("Loader: ", label_style),
            Span::raw(loader.to_string()),
        ]),
        Line::from(vec![
            Span::styled("Loader version: ", label_style),
            Span::raw(loader_version),
        ]),
    ])
    .style(Style::default().fg(THEME.colors.foreground))
    .wrap(Wrap { trim: true })
    .render(area, buf);
}

fn visible_versions(state: &WizardState) -> Vec<GameVersion> {
    let q = state.version_search.query.to_lowercase();
    match &state.versions {
        LoadState::Loaded(versions) => versions
            .iter()
            .filter(|v| state.show_snapshots || v.stable)
            .filter(|v| q.is_empty() || v.id.to_lowercase().contains(&q))
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

fn clamp_version_index(state: &mut WizardState) {
    let count = visible_versions(state).len();
    if count == 0 {
        state.version_idx = 0;
    } else if state.version_idx >= count {
        state.version_idx = count.saturating_sub(1);
    }
}

fn clamp_loader_version_index(state: &mut WizardState) {
    if let LoadState::Loaded(versions) = &state.loader_versions {
        if versions.is_empty() {
            state.loader_version_idx = 0;
        } else if state.loader_version_idx >= versions.len() {
            state.loader_version_idx = versions.len().saturating_sub(1);
        }
    } else {
        state.loader_version_idx = 0;
    }
}

fn ensure_versions_loaded(state: &mut WizardState) {
    if !matches!(state.versions, LoadState::Idle) {
        return;
    }

    state.versions = LoadState::Loading;
    let versions_arc = WIZARD_STATE.clone();
    let loader = state.selected_loader();
    tokio::spawn(async move {
        let client = crate::net::HttpClient::new();
        let installer = get_installer(loader);
        match installer.get_game_versions(&client).await {
            Ok(versions) => match versions_arc.lock() {
                Ok(mut s) => {
                    s.versions = LoadState::Loaded(versions);
                    clamp_version_index(&mut s);
                }
                Err(e) => {
                    tracing::error!("Wizard state lock poisoned: {}", e);
                }
            },
            Err(e) => {
                match versions_arc.lock() {
                    Ok(mut s) => {
                        s.versions = LoadState::Error(e.to_string());
                    }
                    Err(lock_error) => {
                        tracing::error!("Wizard state lock poisoned: {}", lock_error);
                    }
                }
            }
        }
    });
}

fn ensure_loader_versions_loaded(state: &mut WizardState, loader: ModLoader, game_version: String) {
    if !matches!(state.loader_versions, LoadState::Idle) {
        return;
    }

    state.loader_versions = LoadState::Loading;
    let versions_arc = WIZARD_STATE.clone();
    tokio::spawn(async move {
        let client = crate::net::HttpClient::new();
        let installer = get_installer(loader);
        match installer.get_versions(&client, &game_version).await {
            Ok(versions) => match versions_arc.lock() {
                Ok(mut s) => {
                    s.loader_versions = LoadState::Loaded(versions);
                    clamp_loader_version_index(&mut s);
                }
                Err(e) => {
                    tracing::error!("Wizard state lock poisoned: {}", e);
                }
            },
            Err(e) => {
                match versions_arc.lock() {
                    Ok(mut s) => {
                        s.loader_versions = LoadState::Error(e.to_string());
                    }
                    Err(lock_error) => {
                        tracing::error!("Wizard state lock poisoned: {}", lock_error);
                    }
                }
            }
        }
    });
}
