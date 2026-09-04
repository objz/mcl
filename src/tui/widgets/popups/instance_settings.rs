// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// modal editor for settings belonging to the selected Minecraft instance.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use ratatui_textarea::{CursorMove, TextArea};
use std::sync::{Arc, Mutex};

use crate::{
    config::{
        SETTINGS,
        theme::{BORDER_STYLE, THEME},
    },
    instance::loader::GameVersion,
    instance::models::{InstanceConfig, ModLoader, normalize_memory_value, parse_resolution},
    tui::widgets::popups::LoadState,
};

const FIELD_COUNT: usize = 9;
type SharedLoad<T> = Arc<Mutex<LoadState<T>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VersionPicker {
    Game,
    Loader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChoicePicker {
    Loader,
}

enum PickerLoad {
    Idle,
    Loading,
    Loaded,
    Error(String),
}

#[derive(Debug, Clone)]
pub struct State {
    original: InstanceConfig,
    pub draft: InstanceConfig,
    selected: usize,
    editing: Option<TextArea<'static>>,
    pub desktop: bool,
    original_desktop: bool,
    error: Option<String>,
    picker: Option<VersionPicker>,
    picker_index: usize,
    picker_initialized: bool,
    picker_query: String,
    picker_search: bool,
    show_snapshots: bool,
    game_versions: SharedLoad<Vec<GameVersion>>,
    loader_versions: SharedLoad<Vec<String>>,
    choice_picker: Option<ChoicePicker>,
    choice_index: usize,
}

pub enum Action {
    None,
    Save(Box<InstanceConfig>, bool),
    ConfirmRuntime {
        name: String,
        from: String,
        to: String,
    },
    ConfirmClose,
    OpenRaw,
    Close,
}

impl State {
    pub fn new(instance: &InstanceConfig, _meta_dir: &std::path::Path) -> Self {
        Self {
            original: instance.clone(),
            draft: instance.clone(),
            selected: 0,
            editing: None,
            desktop: crate::instance::desktop::exists(&instance.name),
            original_desktop: crate::instance::desktop::exists(&instance.name),
            error: None,
            picker: None,
            picker_index: 0,
            picker_initialized: false,
            picker_query: String::new(),
            picker_search: false,
            show_snapshots: false,
            game_versions: Arc::new(Mutex::new(LoadState::Idle)),
            loader_versions: Arc::new(Mutex::new(LoadState::Idle)),
            choice_picker: None,
            choice_index: 0,
        }
    }

    fn dirty(&self) -> bool {
        self.draft != self.original || self.desktop != self.original_desktop
    }

    fn runtime_changed(&self) -> bool {
        self.draft.game_version != self.original.game_version
            || self.draft.loader != self.original.loader
            || self.draft.loader_version != self.original.loader_version
    }

    fn validate_before_save(&mut self) -> bool {
        self.error = None;
        let settings = SETTINGS.read();
        if self.draft.game_version.trim().is_empty() {
            self.error = Some("game version cannot be empty".to_owned());
        } else if self.draft.loader != ModLoader::Vanilla
            && self
                .draft
                .loader_version
                .as_deref()
                .is_none_or(str::is_empty)
        {
            self.error = Some("the selected loader requires a loader version".to_owned());
        } else if let (Some(min), Some(max)) = (
            memory_kib(
                self.draft
                    .memory_min
                    .as_deref()
                    .unwrap_or(&settings.defaults.memory_min),
            ),
            memory_kib(
                self.draft
                    .memory_max
                    .as_deref()
                    .unwrap_or(&settings.defaults.memory_max),
            ),
        ) && min > max
        {
            self.error = Some("minimum memory cannot exceed maximum memory".to_owned());
        }
        self.error.is_none()
    }

    fn value(&self, field: usize) -> String {
        match field {
            0 => self.draft.game_version.clone(),
            1 => self.draft.loader.to_string(),
            2 => self.draft.loader_version.clone().unwrap_or_default(),
            3 => self.draft.java_path.clone().unwrap_or_default(),
            4 => self.draft.memory_min.clone().unwrap_or_default(),
            5 => self.draft.memory_max.clone().unwrap_or_default(),
            6 => self.draft.jvm_args.join(" "),
            7 => self
                .draft
                .resolution
                .map(|(w, h)| format!("{w}x{h}"))
                .unwrap_or_default(),
            8 => if self.desktop { "yes" } else { "no" }.to_owned(),
            _ => String::new(),
        }
    }

    fn display_value(&self, field: usize) -> String {
        match field {
            2 if self.draft.loader == ModLoader::Vanilla => "not applicable".to_owned(),
            3 if self.draft.java_path.is_none() => "auto-detect".to_owned(),
            4 if self.draft.memory_min.is_none() => {
                format!("default ({})", SETTINGS.read().defaults.memory_min)
            }
            5 if self.draft.memory_max.is_none() => {
                format!("default ({})", SETTINGS.read().defaults.memory_max)
            }
            6 if self.draft.jvm_args.is_empty() => "no arguments".to_owned(),
            6 => self.draft.jvm_args.join(" "),
            7 if self.draft.resolution.is_none() => "default".to_owned(),
            8 if self.desktop => "● enabled".to_owned(),
            8 => "○ disabled".to_owned(),
            _ => self.value(field).replace('\n', " ↵ "),
        }
    }

    fn begin_edit(&mut self) {
        match self.selected {
            0 => self.open_game_picker(),
            1 => self.open_choice_picker(ChoicePicker::Loader),
            2 if self.draft.loader == ModLoader::Vanilla => {
                self.error = Some("Vanilla does not use a loader version".to_owned());
            }
            2 => self.open_loader_picker(),
            3..=7 => self.editing = Some(new_text_area(vec![self.value(self.selected)])),
            8 => self.desktop = !self.desktop,
            field => self.editing = Some(new_text_area(vec![self.value(field)])),
        }
    }

    fn open_choice_picker(&mut self, picker: ChoicePicker) {
        self.choice_picker = Some(picker);
        self.choice_index = match picker {
            ChoicePicker::Loader => loaders()
                .iter()
                .position(|loader| *loader == self.draft.loader)
                .unwrap_or(0),
        };
    }

    fn choice_values(&self) -> Vec<String> {
        self.choice_picker
            .map_or_else(Vec::new, |picker| self.choice_values_for(picker))
    }

    fn choice_values_for(&self, picker: ChoicePicker) -> Vec<String> {
        match picker {
            ChoicePicker::Loader => loaders().iter().map(ToString::to_string).collect(),
        }
    }

    fn handle_choice_key(&mut self, key: &KeyEvent) {
        let count = self.choice_values().len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => self.choice_picker = None,
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Right if count > 0 => {
                self.choice_index = (self.choice_index + 1).min(count - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.choice_index = self.choice_index.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => self.apply_choice(),
            _ => {}
        }
    }

    fn apply_choice(&mut self) {
        match self.choice_picker {
            Some(ChoicePicker::Loader) => {
                let available = loaders();
                let loader = available[self.choice_index.min(available.len() - 1)];
                if self.draft.loader != loader {
                    self.draft.loader = loader;
                    self.draft.loader_version = None;
                    self.loader_versions = Arc::new(Mutex::new(LoadState::Idle));
                    self.game_versions = Arc::new(Mutex::new(LoadState::Idle));
                }
            }
            None => {}
        }
        self.choice_picker = None;
    }

    fn open_game_picker(&mut self) {
        self.picker = Some(VersionPicker::Game);
        self.picker_index = 0;
        self.picker_initialized = false;
        self.picker_query.clear();
        self.picker_search = false;
        let mut load = self
            .game_versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(*load, LoadState::Idle | LoadState::Error(_)) {
            *load = LoadState::Loading;
            let target = self.game_versions.clone();
            let loader = self.draft.loader;
            tokio::spawn(async move {
                let result = super::version_lists::game_versions(loader).await;
                *target
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = match result {
                    Ok(versions) => LoadState::Loaded(versions),
                    Err(error) => LoadState::Error(error),
                };
                crate::feedback::request_redraw();
            });
        }
    }

    fn open_loader_picker(&mut self) {
        self.picker = Some(VersionPicker::Loader);
        self.picker_index = 0;
        self.picker_initialized = false;
        self.picker_query.clear();
        self.picker_search = false;
        let mut load = self
            .loader_versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(*load, LoadState::Idle | LoadState::Error(_)) {
            *load = LoadState::Loading;
            let target = self.loader_versions.clone();
            let loader = self.draft.loader;
            let game_version = self.draft.game_version.clone();
            tokio::spawn(async move {
                let result = super::version_lists::loader_versions(loader, &game_version).await;
                *target
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = match result {
                    Ok(versions) => LoadState::Loaded(versions),
                    Err(error) => LoadState::Error(error),
                };
                crate::feedback::request_redraw();
            });
        }
    }

    fn visible_game_versions(&self) -> Vec<GameVersion> {
        let query = self.picker_query.to_lowercase();
        match &*self
            .game_versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            LoadState::Loaded(versions) => versions
                .iter()
                .filter(|version| self.show_snapshots || version.stable)
                .filter(|version| query.is_empty() || version.id.to_lowercase().contains(&query))
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }

    fn visible_loader_versions(&self) -> Vec<String> {
        let query = self.picker_query.to_lowercase();
        match &*self
            .loader_versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            LoadState::Loaded(versions) => versions
                .iter()
                .filter(|version| query.is_empty() || version.to_lowercase().contains(&query))
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }

    fn initialize_picker_index(&mut self) {
        if self.picker_initialized {
            return;
        }
        let index = match self.picker {
            Some(VersionPicker::Game) => self
                .visible_game_versions()
                .iter()
                .position(|version| version.id == self.draft.game_version),
            Some(VersionPicker::Loader) => {
                self.draft.loader_version.as_deref().and_then(|selected| {
                    self.visible_loader_versions()
                        .iter()
                        .position(|version| version == selected)
                })
            }
            None => None,
        };
        if let Some(index) = index {
            self.picker_index = index;
            self.picker_initialized = true;
        }
    }

    fn handle_picker_key(&mut self, key: &KeyEvent) {
        self.initialize_picker_index();
        if self.picker_search {
            match key.code {
                KeyCode::Esc => {
                    self.picker_search = false;
                    return;
                }
                KeyCode::Backspace => {
                    self.picker_query.pop();
                    self.picker_index = 0;
                    return;
                }
                KeyCode::Char('j') | KeyCode::Down | KeyCode::Char('k') | KeyCode::Up => {}
                KeyCode::Char(character) => {
                    self.picker_query.push(character);
                    self.picker_index = 0;
                    return;
                }
                _ => {}
            }
        }
        let count = match self.picker {
            Some(VersionPicker::Game) => self.visible_game_versions().len(),
            Some(VersionPicker::Loader) => self.visible_loader_versions().len(),
            None => 0,
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => self.picker = None,
            KeyCode::Char('/') => self.picker_search = true,
            KeyCode::Char('s') if self.picker == Some(VersionPicker::Game) => {
                self.show_snapshots = !self.show_snapshots;
                self.picker_index = 0;
                self.picker_initialized = false;
            }
            KeyCode::Char('j') | KeyCode::Down if count > 0 => {
                self.picker_index = (self.picker_index + 1).min(count - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.picker_index = self.picker_index.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => match self.picker {
                Some(VersionPicker::Game) => {
                    if let Some(version) = self.visible_game_versions().get(self.picker_index) {
                        if self.draft.game_version != version.id {
                            self.draft.game_version = version.id.clone();
                            self.draft.loader_version = None;
                            self.loader_versions = Arc::new(Mutex::new(LoadState::Idle));
                        }
                        self.picker = None;
                    }
                }
                Some(VersionPicker::Loader) => {
                    if let Some(version) = self.visible_loader_versions().get(self.picker_index) {
                        self.draft.loader_version = Some(version.clone());
                        self.picker = None;
                    }
                }
                None => {}
            },
            _ => {}
        }
    }

    fn commit_edit(&mut self) {
        let Some(editor) = self.editing.take() else {
            return;
        };
        let raw = editor.lines().join("\n");
        let value = raw.trim();
        self.error = None;
        let invalid = |state: &mut Self, message: String| {
            state.error = Some(message);
            state.editing = Some(new_text_area(editor.lines().to_vec()));
        };
        match self.selected {
            0 if value.is_empty() => invalid(self, "game version cannot be empty".to_owned()),
            0 => self.draft.game_version = value.to_owned(),
            2 => self.draft.loader_version = (!value.is_empty()).then(|| value.to_owned()),
            3 => self.draft.java_path = (!value.is_empty()).then(|| value.to_owned()),
            4 | 5 if !value.is_empty() && normalize_memory_value(value).is_none() => invalid(
                self,
                "memory must be a positive number with K, M, or G".to_owned(),
            ),
            4 => self.draft.memory_min = normalize_memory_value(value),
            5 => self.draft.memory_max = normalize_memory_value(value),
            6 => {
                self.draft.jvm_args = value.split_whitespace().map(str::to_owned).collect();
            }
            7 if value.is_empty() => self.draft.resolution = None,
            7 => match parse_resolution(value) {
                Ok(resolution) => self.draft.resolution = Some(resolution),
                Err(error) => invalid(self, error),
            },
            _ => {}
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> Action {
        if self.choice_picker.is_some() {
            self.handle_choice_key(key);
            return Action::None;
        }
        if self.picker.is_some() {
            self.handle_picker_key(key);
            return Action::None;
        }
        if let Some(input) = &mut self.editing {
            match key.code {
                KeyCode::Enter => self.commit_edit(),
                KeyCode::Esc => self.editing = None,
                _ => {
                    input.input(*key);
                }
            }
            return Action::None;
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.selected = (self.selected + 1).min(FIELD_COUNT - 1)
            }
            KeyCode::Char('k') | KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Enter => self.begin_edit(),
            KeyCode::Char('s') if self.dirty() => {
                if self.validate_before_save() {
                    if self.runtime_changed() {
                        return Action::ConfirmRuntime {
                            name: self.draft.name.clone(),
                            from: runtime_label(&self.original),
                            to: runtime_label(&self.draft),
                        };
                    } else {
                        return Action::Save(Box::new(self.draft.clone()), self.desktop);
                    }
                }
            }
            KeyCode::Char('E') if self.dirty() => {
                self.error =
                    Some("save or discard draft changes before opening the raw file".to_owned());
            }
            KeyCode::Char('E') => return Action::OpenRaw,
            KeyCode::Esc if self.dirty() => return Action::ConfirmClose,
            KeyCode::Esc => return Action::Close,
            _ => {}
        }
        Action::None
    }

    pub fn confirmed_save(&mut self) -> Option<(Box<InstanceConfig>, bool)> {
        self.validate_before_save()
            .then(|| (Box::new(self.draft.clone()), self.desktop))
    }
}

fn runtime_label(config: &InstanceConfig) -> String {
    if config.loader == ModLoader::Vanilla {
        format!("{} / Vanilla", config.game_version)
    } else {
        format!(
            "{} / {} {}",
            config.game_version,
            config.loader,
            config.loader_version.as_deref().unwrap_or("unknown")
        )
    }
}

fn memory_kib(value: &str) -> Option<u64> {
    let normalized = normalize_memory_value(value)?;
    let (number, suffix) = normalized.split_at(normalized.len().saturating_sub(1));
    let number = number.parse::<u64>().ok()?;
    match suffix {
        "K" => Some(number),
        "M" => number.checked_mul(1024),
        "G" => number.checked_mul(1024 * 1024),
        _ => None,
    }
}

fn loaders() -> [ModLoader; 5] {
    [
        ModLoader::Vanilla,
        ModLoader::Fabric,
        ModLoader::Forge,
        ModLoader::NeoForge,
        ModLoader::Quilt,
    ]
}

fn new_text_area(lines: Vec<String>) -> TextArea<'static> {
    let theme = THEME.as_ref();
    let mut editor = TextArea::new(if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    });
    editor.set_style(Style::default().fg(theme.text()).bg(theme.surface()));
    editor.set_cursor_line_style(Style::default());
    editor.set_cursor_style(Style::default().fg(theme.background()).bg(theme.accent()));
    editor.move_cursor(CursorMove::Bottom);
    editor.move_cursor(CursorMove::End);
    editor
}

pub fn popup_rect(area: Rect, state: &State) -> Rect {
    let height = if state.picker.is_some() {
        (area.height * 2 / 3).max(10)
    } else if state.choice_picker.is_some() {
        7
    } else {
        11 + u16::from(state.error.is_some())
    };
    area.centered(
        Constraint::Percentage(58),
        Constraint::Length(height.min(area.height.saturating_sub(4))),
    )
}

pub fn render(frame: &mut Frame, area: Rect, state: &State) {
    let theme = THEME.as_ref();
    frame.render_widget(Clear, area);
    let title = if state.dirty() {
        " Instance Settings * "
    } else {
        " Instance Settings "
    };
    let keybinds = if state.editing.is_some() {
        super::keybind_line(&[("Enter", " apply"), ("Esc", " cancel")])
    } else if state.picker.is_some() {
        super::keybind_line(&[
            ("/", " search"),
            ("s", " snap"),
            ("h", " back"),
            ("Enter", " select"),
        ])
    } else if state.choice_picker.is_some() {
        super::keybind_line(&[("h", " back"), ("Enter", " select")])
    } else {
        super::keybind_line(&[
            ("j/k", ""),
            ("Enter", " edit"),
            ("s", " save"),
            ("E", " raw"),
            ("Esc", " back"),
        ])
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BORDER_STYLE.to_border_type())
        .border_style(Style::default().fg(theme.text_dim()))
        .style(Style::default().bg(theme.surface()))
        .title_bottom(keybinds.right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.picker.is_some() {
        render_version_picker(frame, inner, state);
        return;
    }
    if state.choice_picker.is_some() {
        render_choice_picker(frame, inner, state);
        return;
    }

    render_settings_list(frame, inner, state);
}

fn render_settings_list(frame: &mut Frame, area: Rect, state: &State) {
    let theme = THEME.as_ref();
    let labels = [
        "Game version",
        "Loader",
        "Loader version",
        "Java",
        "Memory min",
        "Memory max",
        "JVM args",
        "Resolution",
        "Desktop shortcut",
    ];
    let lines = labels
        .iter()
        .enumerate()
        .map(|(index, label)| field_line(state, index, label))
        .chain(state.error.iter().map(|error| {
            Line::from(Span::styled(
                format!("  {error}"),
                Style::default().fg(theme.error()),
            ))
        }))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);

    if let Some(editor) = state.editing.as_ref() {
        let edit_area = Rect {
            x: area.x.saturating_add(20),
            y: area.y.saturating_add(state.selected as u16),
            width: area.width.saturating_sub(20),
            height: 1,
        };
        frame.render_widget(editor, edit_area);
    }
}

fn field_line<'a>(state: &'a State, index: usize, label: &str) -> Line<'a> {
    let theme = THEME.as_ref();
    let selected = index == state.selected;
    let editing = selected && state.editing.is_some();
    let value = if editing {
        String::new()
    } else {
        state.display_value(index)
    };
    let dirty = field_dirty(state, index);
    Line::from(vec![
        Span::styled(
            if selected { "▶ " } else { "  " },
            Style::default().fg(theme.accent()),
        ),
        Span::styled(
            format!("{label:<18}"),
            Style::default().fg(theme.text_dim()),
        ),
        Span::styled(
            value,
            Style::default()
                .fg(if index == 8 && state.desktop {
                    theme.success()
                } else if selected {
                    theme.accent()
                } else {
                    theme.text()
                })
                .add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ),
        Span::styled(
            if dirty { " *" } else { "" },
            Style::default().fg(theme.accent()),
        ),
    ])
}

fn field_dirty(state: &State, index: usize) -> bool {
    match index {
        0 => state.draft.game_version != state.original.game_version,
        1 => state.draft.loader != state.original.loader,
        2 => state.draft.loader_version != state.original.loader_version,
        3 => state.draft.java_path != state.original.java_path,
        4 => state.draft.memory_min != state.original.memory_min,
        5 => state.draft.memory_max != state.original.memory_max,
        6 => state.draft.jvm_args != state.original.jvm_args,
        7 => state.draft.resolution != state.original.resolution,
        8 => state.desktop != state.original_desktop,
        _ => false,
    }
}

fn render_choice_picker(frame: &mut Frame, area: Rect, state: &State) {
    let theme = THEME.as_ref();
    let values = state.choice_values();
    let visible_rows = area.height as usize;
    let start = state
        .choice_index
        .saturating_sub(visible_rows.saturating_sub(1));
    let mut lines = Vec::new();
    for (index, value) in values.iter().enumerate().skip(start).take(visible_rows) {
        let selected = index == state.choice_index;
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▶ " } else { "  " },
                Style::default().fg(theme.accent()),
            ),
            Span::styled(
                value.clone(),
                Style::default()
                    .fg(if selected {
                        theme.accent()
                    } else {
                        theme.text()
                    })
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_version_picker(frame: &mut Frame, area: Rect, state: &State) {
    let theme = THEME.as_ref();
    let status = match state.picker {
        Some(VersionPicker::Game) => match &*state
            .game_versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            LoadState::Idle => PickerLoad::Idle,
            LoadState::Loading => PickerLoad::Loading,
            LoadState::Loaded(_) => PickerLoad::Loaded,
            LoadState::Error(error) => PickerLoad::Error(error.clone()),
        },
        Some(VersionPicker::Loader) => match &*state
            .loader_versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            LoadState::Idle => PickerLoad::Idle,
            LoadState::Loading => PickerLoad::Loading,
            LoadState::Loaded(_) => PickerLoad::Loaded,
            LoadState::Error(error) => PickerLoad::Error(error.clone()),
        },
        None => return,
    };
    let mut lines = vec![Line::from(vec![
        Span::styled("Search  ", Style::default().fg(theme.text_dim())),
        Span::styled(
            if state.picker_search {
                format!("/{}█", state.picker_query)
            } else if state.picker_query.is_empty() {
                "press / to search".to_owned()
            } else {
                format!("/{}", state.picker_query)
            },
            Style::default().fg(theme.accent()),
        ),
    ])];
    match status {
        PickerLoad::Idle | PickerLoad::Loading => lines.push(Line::from("Loading versions...")),
        PickerLoad::Error(error) => lines.push(Line::from(Span::styled(
            format!("Failed to load versions: {error}. Reopen to retry."),
            Style::default().fg(theme.error()),
        ))),
        PickerLoad::Loaded => {
            let versions: Vec<String> = match state.picker {
                Some(VersionPicker::Game) => state
                    .visible_game_versions()
                    .into_iter()
                    .map(|version| {
                        if version.stable {
                            version.id
                        } else {
                            format!("{} (snapshot)", version.id)
                        }
                    })
                    .collect(),
                Some(VersionPicker::Loader) => state.visible_loader_versions(),
                None => Vec::new(),
            };
            if versions.is_empty() {
                lines.push(Line::from("No matching versions."));
            } else {
                let visible_rows = area.height.saturating_sub(1) as usize;
                let start = state
                    .picker_index
                    .saturating_sub(visible_rows.saturating_sub(1));
                for (index, version) in versions.iter().enumerate().skip(start).take(visible_rows) {
                    let selected = index == state.picker_index;
                    lines.push(Line::from(vec![
                        Span::styled(
                            if selected { "▶ " } else { "  " },
                            Style::default().fg(theme.accent()),
                        ),
                        Span::styled(
                            version.clone(),
                            Style::default()
                                .fg(if selected {
                                    theme.accent()
                                } else {
                                    theme.text()
                                })
                                .add_modifier(if selected {
                                    Modifier::BOLD
                                } else {
                                    Modifier::empty()
                                }),
                        ),
                    ]));
                }
            }
        }
    }
    frame.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn instance() -> InstanceConfig {
        InstanceConfig {
            name: "test".to_owned(),
            game_version: "1.21.1".to_owned(),
            loader: ModLoader::Fabric,
            loader_version: Some("0.16.0".to_owned()),
            created: Utc::now(),
            last_played: None,
            java_path: None,
            memory_max: None,
            memory_min: None,
            jvm_args: Vec::new(),
            resolution: None,
            config_sync_profile: None,
            modpack_source: None,
        }
    }

    #[test]
    fn memory_and_resolution_inputs_are_normalized() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());
        state.selected = 4;
        state.editing = Some(new_text_area(vec!["2048m".to_owned()]));
        state.commit_edit();
        assert_eq!(state.draft.memory_min.as_deref(), Some("2048M"));

        state.selected = 7;
        state.editing = Some(new_text_area(vec!["1920X1080".to_owned()]));
        state.commit_edit();
        assert_eq!(state.draft.resolution, Some((1920, 1080)));
    }

    #[test]
    fn memory_and_jvm_arguments_are_edited_inline() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = instance();
        config.memory_min = Some("512M".to_owned());
        let mut state = State::new(&config, temp.path());
        state.selected = 4;
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        state.handle_key(&KeyEvent::from(KeyCode::Left));
        state.handle_key(&KeyEvent::from(KeyCode::Char('0')));
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert_eq!(state.draft.memory_min.as_deref(), Some("5120M"));

        state.selected = 6;
        state.begin_edit();
        for character in "-Xfoo -Xbar".chars() {
            state.handle_key(&KeyEvent::from(KeyCode::Char(character)));
        }
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert_eq!(state.draft.jvm_args, ["-Xfoo", "-Xbar"]);
    }

    #[test]
    fn runtime_changes_require_confirmation_before_save() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());
        state.draft.game_version = "1.21.2".to_owned();

        assert!(matches!(
            state.handle_key(&KeyEvent::from(KeyCode::Char('s'))),
            Action::ConfirmRuntime { .. }
        ));
        assert!(state.confirmed_save().is_some());
    }

    #[test]
    fn version_picker_selects_loaded_version_and_clears_loader_version() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());
        *state.game_versions.lock().unwrap() = LoadState::Loaded(vec![
            GameVersion {
                id: "1.21.2".to_owned(),
                stable: true,
            },
            GameVersion {
                id: "1.21.1".to_owned(),
                stable: true,
            },
        ]);
        state.picker = Some(VersionPicker::Game);
        state.picker_initialized = true;
        state.picker_index = 0;

        state.handle_picker_key(&KeyEvent::from(KeyCode::Enter));

        assert_eq!(state.draft.game_version, "1.21.2");
        assert_eq!(state.draft.loader_version, None);
    }

    #[test]
    fn popup_changes_do_not_replace_the_existing_config_profile() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = instance();
        config.config_sync_profile = Some("shared".to_owned());
        let mut state = State::new(&config, temp.path());
        state.desktop = !state.desktop;

        let Action::Save(config, _) = state.handle_key(&KeyEvent::from(KeyCode::Char('s'))) else {
            panic!("expected settings save");
        };

        assert_eq!(config.config_sync_profile.as_deref(), Some("shared"));
    }

    #[test]
    fn selecting_loader_clears_the_previous_loaders_version() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());

        state.open_choice_picker(ChoicePicker::Loader);
        state.handle_choice_key(&KeyEvent::from(KeyCode::Char('j')));
        state.handle_choice_key(&KeyEvent::from(KeyCode::Enter));

        assert_eq!(state.draft.loader, ModLoader::Forge);
        assert_eq!(state.draft.loader_version, None);
        assert!(!state.validate_before_save());
    }

    #[test]
    fn loader_picker_and_desktop_toggle_use_enter() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());

        state.selected = 1;
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        state.handle_key(&KeyEvent::from(KeyCode::Char('j')));
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert_eq!(state.draft.loader, ModLoader::Forge);

        state.selected = 8;
        let desktop = state.desktop;
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert_ne!(state.desktop, desktop);
    }

    #[test]
    fn text_fields_open_inline_editors() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());

        state.selected = 4;
        state.begin_edit();
        assert!(state.editing.is_some());
        assert!(state.choice_picker.is_none());
        assert!(state.picker.is_none());
    }
}
