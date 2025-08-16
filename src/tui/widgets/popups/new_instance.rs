use super::base::PopupFrame;
use crate::instance::{loader::get_installer, models::ModLoader};
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
    pub versions: LoadState<Vec<crate::net::mojang::VersionEntry>>,
    pub version_idx: usize,
    pub show_snapshots: bool,
    pub loader_idx: usize,
    pub loader_versions: LoadState<Vec<String>>,
    pub loader_version_idx: usize,
    pub version_search: String,
    pub version_search_active: bool,
}

impl WizardState {
    pub fn reset(&mut self) {
        *self = WizardState::default();
    }

    pub fn selected_version(&self) -> Option<&crate::net::mojang::VersionEntry> {
        if let LoadState::Loaded(ref versions) = self.versions {
            let visible: Vec<_> = versions
                .iter()
                .filter(|v| self.show_snapshots || v.version_type == "release")
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

    let popup = PopupFrame {
        title: wizard_title(&snapshot),
        border_color: THEME.colors.border_focused,
        bg: Some(THEME.colors.row_alternate_bg),
        keybinds: Some(keybinds),
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
        WizardStep::Loader => handle_loader_key(&mut state, key_event),
        WizardStep::LoaderVersion => handle_loader_version_key(&mut state, key_event),
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
    let step = match WIZARD_STATE.lock() {
        Ok(s) => s.step.clone(),
        Err(_) => WizardStep::Name,
    };

    let popup_w = frame_area.width / 2;
    let x = frame_area.width / 4;

    match step {
        WizardStep::Name => {
            let h = 6u16.min(frame_area.height.saturating_sub(4));
            let y = (frame_area.height.saturating_sub(h)) / 2;
            Rect { x, y, width: popup_w, height: h }
        }
        WizardStep::Version | WizardStep::LoaderVersion => {
            let h = (frame_area.height * 2 / 3).max(10).min(frame_area.height.saturating_sub(4));
            let y = (frame_area.height.saturating_sub(h)) / 2;
            Rect { x, y, width: popup_w, height: h }
        }
        WizardStep::Loader => {
            let h = 9u16.min(frame_area.height.saturating_sub(4));
            let y = (frame_area.height.saturating_sub(h)) / 2;
            Rect { x, y, width: popup_w, height: h }
        }
        WizardStep::Confirm => {
            let h = 8u16.min(frame_area.height.saturating_sub(4));
            let y = (frame_area.height.saturating_sub(h)) / 2;
            Rect { x, y, width: popup_w, height: h }
        }
    }
}

fn handle_name_key(
    state: &mut WizardState,
    key_event: &KeyEvent,
    profiles_state: &mut profiles::State,
) {
    match key_event.code {
        KeyCode::Esc => close_popup(state, profiles_state),
        KeyCode::Enter => {
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
    if state.version_search_active {
        match key_event.code {
            KeyCode::Esc => {
                state.version_search_active = false;
                state.version_search.clear();
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
            if state.version_search_active {
                state.version_search_active = false;
                state.version_search.clear();
                clamp_version_index(state);
            } else {
                state.step = WizardStep::Loader;
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if visible_count > 0 {
                state.version_idx = (state.version_idx + 1).min(visible_count.saturating_sub(1));
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.version_idx = state.version_idx.saturating_sub(1);
        }
        KeyCode::Char('s') => {
            state.show_snapshots = !state.show_snapshots;
            clamp_version_index(state);
        }
        KeyCode::Char('/') if !state.version_search_active => {
            state.version_search_active = true;
            state.version_idx = 0;
        }
        KeyCode::Enter => {
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
        KeyCode::Char('q') => close_popup(state, profiles_state),
        _ => {}
    }
}

fn handle_loader_key(state: &mut WizardState, key_event: &KeyEvent) {
    match key_event.code {
        KeyCode::Esc => state.step = WizardStep::Name,
        KeyCode::Char('j') | KeyCode::Down => {
            state.loader_idx = (state.loader_idx + 1).min(4);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.loader_idx = state.loader_idx.saturating_sub(1);
        }
        KeyCode::Enter => {
            state.versions = LoadState::Idle;
            state.version_idx = 0;
            state.version_search.clear();
            state.version_search_active = false;
            state.step = WizardStep::Version;
            ensure_versions_loaded(state);
        }
        _ => {}
    }
}

fn handle_loader_version_key(state: &mut WizardState, key_event: &KeyEvent) {
    if state.selected_loader() == ModLoader::Vanilla {
        state.step = WizardStep::Confirm;
        return;
    }

    let version_count = match &state.loader_versions {
        LoadState::Loaded(versions) => versions.len(),
        _ => 0,
    };

    match key_event.code {
        KeyCode::Esc => state.step = WizardStep::Version,
        KeyCode::Char('j') | KeyCode::Down => {
            if version_count > 0 {
                state.loader_version_idx =
                    (state.loader_version_idx + 1).min(version_count.saturating_sub(1));
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            state.loader_version_idx = state.loader_version_idx.saturating_sub(1);
        }
        KeyCode::Enter => {
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
        KeyCode::Esc => {
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
        WizardStep::Name => keybind_line(&[("Enter", " continue"), ("Esc", " close")]),
        WizardStep::Version => keybind_line(&[("j/k", " move"), ("/", " search"), ("s", " snapshots"), ("Enter", " select"), ("Esc", " back")]),
        WizardStep::Loader => keybind_line(&[("j/k", " move"), ("Enter", " select"), ("Esc", " back")]),
        WizardStep::LoaderVersion => keybind_line(&[("j/k", " move"), ("Enter", " select"), ("Esc", " back")]),
        WizardStep::Confirm => keybind_line(&[("Enter", " create"), ("Esc", " back")]),
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
            // Split area for search bar (when active) + list
            let (search_area, list_area) = if state.version_search_active || !state.version_search.is_empty() {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Min(1)])
                    .split(area);
                (Some(chunks[0]), chunks[1])
            } else {
                (None, area)
            };

            if let Some(sa) = search_area {
                let search_display = format!("/ {}█", state.version_search);
                Paragraph::new(search_display.as_str())
                    .style(Style::default().fg(THEME.colors.border_focused))
                    .render(sa, buf);
            }

            let items: Vec<ListItem> = visible_versions(state)
                .into_iter()
                .map(|version| {
                    let suffix = if version.version_type == "release" {
                        String::new()
                    } else {
                        format!(" ({})", version.version_type)
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
            StatefulWidget::render(list, list_area, buf, &mut list_state);
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

fn visible_versions(state: &WizardState) -> Vec<crate::net::mojang::VersionEntry> {
    let q = state.version_search.to_lowercase();
    let loader = state.selected_loader();
    match &state.versions {
        LoadState::Loaded(versions) => versions
            .iter()
            .filter(|v| state.show_snapshots || v.version_type == "release")
            .filter(|v| q.is_empty() || v.id.to_lowercase().contains(&q))
            .filter(|v| is_version_loader_compatible(&v.id, loader))
            .cloned()
            .collect(),
        _ => Vec::new(),
    }
}

fn is_version_loader_compatible(version_id: &str, loader: ModLoader) -> bool {
    let parts: Vec<u32> = version_id
        .split('.')
        .take(2)
        .filter_map(|s| s.parse().ok())
        .collect();

    let (major, minor) = match parts.as_slice() {
        [maj, min, ..] => (*maj, *min),
        [maj] => (*maj, 0),
        _ => return false,
    };

    match loader {
        ModLoader::Vanilla | ModLoader::Forge => true,
        ModLoader::Fabric | ModLoader::Quilt => {
            major > 1 || (major == 1 && minor >= 14)
        }
        ModLoader::NeoForge => major == 1 && minor >= 20,
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
    tokio::spawn(async move {
        let client = crate::net::HttpClient::new();
        match crate::net::mojang::fetch_version_manifest(&client).await {
            Ok(manifest) => match versions_arc.lock() {
                Ok(mut s) => {
                    s.versions = LoadState::Loaded(manifest.versions);
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
