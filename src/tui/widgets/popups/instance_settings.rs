// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// modal editor for settings belonging to the selected Minecraft instance.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, ListItem, Paragraph},
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
    tui::widgets::{
        popups::{
            LoadState,
            settings_controls::{
                DisplayResolution, JavaChoice, JavaPicker, adjust_memory, auto_label,
                display_resolutions, handle_text_area_input, memory_kib, render_memory_gauge,
            },
        },
        search::SearchState,
    },
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
    Java,
    Resolution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResolutionChoice {
    Display(DisplayResolution),
    Preset(u32, u32),
    Configured(u32, u32),
}

impl ResolutionChoice {
    fn resolution(&self) -> Option<(u32, u32)> {
        match self {
            Self::Display(display) => Some((display.width, display.height)),
            Self::Preset(width, height) | Self::Configured(width, height) => {
                Some((*width, *height))
            }
        }
    }

    fn label(&self) -> String {
        self.resolution()
            .map(|(width, height)| format!("{width}x{height}"))
            .unwrap_or_default()
    }
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
    picker_search: SearchState,
    show_snapshots: bool,
    game_versions: SharedLoad<Vec<GameVersion>>,
    loader_versions: SharedLoad<Vec<String>>,
    choice_picker: Option<ChoicePicker>,
    choice_index: usize,
    java_picker: JavaPicker,
    display_resolutions: Vec<DisplayResolution>,
}

pub enum Action {
    None,
    Save(Box<InstanceConfig>, bool),
    Error(String),
    ConfirmRuntime { name: String },
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
            picker_search: SearchState::default(),
            show_snapshots: false,
            game_versions: Arc::new(Mutex::new(LoadState::Idle)),
            loader_versions: Arc::new(Mutex::new(LoadState::Idle)),
            choice_picker: None,
            choice_index: 0,
            java_picker: JavaPicker::new(),
            display_resolutions: display_resolutions(),
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
            self.error = Some("Game version is required.".to_owned());
        } else if self.draft.loader != ModLoader::Vanilla
            && self
                .draft
                .loader_version
                .as_deref()
                .is_none_or(str::is_empty)
        {
            self.error = Some("Select a loader version.".to_owned());
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
            self.error = Some("Minimum memory cannot exceed maximum memory.".to_owned());
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
            3 if self.draft.java_path.is_none() => self.java_picker.detected_path().to_owned(),
            4 if self.draft.memory_min.is_none() => SETTINGS.read().defaults.memory_min.clone(),
            5 if self.draft.memory_max.is_none() => SETTINGS.read().defaults.memory_max.clone(),
            6 if self.draft.jvm_args.is_empty() => "no arguments".to_owned(),
            6 => self.draft.jvm_args.join(" "),
            7 if self.draft.resolution.is_none() => self.default_resolution().map_or_else(
                || "not detected".to_owned(),
                |(width, height)| format!("{width}x{height}"),
            ),
            8 if self.desktop => "enabled".to_owned(),
            8 => "disabled".to_owned(),
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
            3 => self.open_choice_picker(ChoicePicker::Java),
            4 | 5 => {
                self.editing = Some(new_text_area(vec![self.effective_memory(self.selected)]));
            }
            6 => self.editing = Some(new_text_area(vec![self.value(self.selected)])),
            7 => self.open_choice_picker(ChoicePicker::Resolution),
            8 => self.desktop = !self.desktop,
            field => self.editing = Some(new_text_area(vec![self.value(field)])),
        }
    }

    fn open_choice_picker(&mut self, picker: ChoicePicker) {
        self.choice_picker = Some(picker);
        self.choice_index = match picker {
            ChoicePicker::Loader => super::select_list::MOD_LOADERS
                .iter()
                .position(|loader| *loader == self.draft.loader)
                .unwrap_or(0),
            ChoicePicker::Java => {
                self.java_picker.open(self.draft.java_path.as_deref());
                self.java_picker.initialize();
                self.java_picker.selected
            }
            ChoicePicker::Resolution => self
                .resolution_choices()
                .iter()
                .position(|choice| choice.resolution() == self.draft.resolution)
                .unwrap_or(0),
        };
    }

    fn choice_values(&self) -> Vec<String> {
        self.choice_picker
            .map_or_else(Vec::new, |picker| self.choice_values_for(picker))
    }

    fn choice_values_for(&self, picker: ChoicePicker) -> Vec<String> {
        match picker {
            ChoicePicker::Loader => super::select_list::MOD_LOADERS
                .iter()
                .map(ToString::to_string)
                .collect(),
            ChoicePicker::Java => self.java_picker.labels(),
            ChoicePicker::Resolution => self
                .resolution_choices()
                .iter()
                .map(ResolutionChoice::label)
                .collect(),
        }
    }

    fn handle_choice_key(&mut self, key: &KeyEvent) {
        if self.choice_picker == Some(ChoicePicker::Java) {
            self.java_picker.initialize();
            self.choice_index = self.java_picker.selected;
        }
        let count = self.choice_values().len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => self.choice_picker = None,
            KeyCode::Char('a') if self.choice_picker == Some(ChoicePicker::Java) => {
                self.toggle_auto_java();
                self.choice_picker = None;
            }
            KeyCode::Char('d') if self.choice_picker == Some(ChoicePicker::Resolution) => {
                self.apply_default_resolution();
                self.choice_picker = None;
            }
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Right if count > 0 => {
                self.choice_index = (self.choice_index + 1).min(count - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.choice_index = self.choice_index.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => self.apply_choice(),
            _ => {}
        }
        if self.choice_picker == Some(ChoicePicker::Java) {
            self.java_picker.selected = self.choice_index;
        }
    }

    fn apply_choice(&mut self) {
        let mut open_loader_versions = false;
        match self.choice_picker {
            Some(ChoicePicker::Loader) => {
                let available = super::select_list::MOD_LOADERS;
                let loader = available[self.choice_index.min(available.len() - 1)];
                if self.draft.loader != loader {
                    self.draft.loader = loader;
                    self.draft.loader_version = None;
                    self.loader_versions = Arc::new(Mutex::new(LoadState::Idle));
                    self.game_versions = Arc::new(Mutex::new(LoadState::Idle));
                    open_loader_versions = loader != ModLoader::Vanilla;
                }
            }
            Some(ChoicePicker::Java) => {
                self.java_picker.selected = self.choice_index;
                match self.java_picker.selected_choice() {
                    JavaChoice::Installation(path) => self.draft.java_path = Some(path),
                }
            }
            Some(ChoicePicker::Resolution) => {
                let selected = self
                    .resolution_choices()
                    .get(self.choice_index)
                    .cloned()
                    .and_then(|choice| choice.resolution());
                if let Some(resolution) = selected {
                    self.draft.resolution = Some(resolution);
                }
            }
            None => {}
        }
        self.choice_picker = None;
        if open_loader_versions {
            self.open_loader_picker();
        }
    }

    fn open_game_picker(&mut self) {
        self.picker = Some(VersionPicker::Game);
        self.picker_index = 0;
        self.picker_initialized = false;
        self.picker_search.deactivate();
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
        self.picker_search.deactivate();
        let mut load = self
            .loader_versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(*load, LoadState::Idle | LoadState::Error(_)) {
            *load = LoadState::Loading;
            let target = self.loader_versions.clone();
            let loader = self.draft.loader;
            let game_version = self.draft.game_version.clone();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let result = super::version_lists::loader_versions(loader, &game_version).await;
                    *target
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = match result {
                        Ok(versions) => LoadState::Loaded(versions),
                        Err(error) => LoadState::Error(error),
                    };
                    crate::feedback::request_redraw();
                });
            } else {
                *load = LoadState::Idle;
            }
        }
    }

    fn visible_game_versions(&self) -> Vec<GameVersion> {
        match &*self
            .game_versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            LoadState::Loaded(versions) => versions
                .iter()
                .filter(|version| self.show_snapshots || version.stable)
                .filter(|version| self.picker_search.matches(&version.id))
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }

    fn visible_loader_versions(&self) -> Vec<String> {
        match &*self
            .loader_versions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            LoadState::Loaded(versions) => versions
                .iter()
                .filter(|version| self.picker_search.matches(version))
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

    fn effective_memory(&self, field: usize) -> String {
        let settings = SETTINGS.read();
        if field == 4 {
            self.draft
                .memory_min
                .clone()
                .unwrap_or_else(|| settings.defaults.memory_min.clone())
        } else {
            self.draft
                .memory_max
                .clone()
                .unwrap_or_else(|| settings.defaults.memory_max.clone())
        }
    }

    fn set_memory(&mut self, field: usize, value: Option<String>) {
        if field == 4 {
            self.draft.memory_min = value.clone();
        } else {
            self.draft.memory_max = value.clone();
        }

        let min = self.effective_memory(4);
        let max = self.effective_memory(5);
        if memory_kib(&min)
            .zip(memory_kib(&max))
            .is_some_and(|(min, max)| min > max)
        {
            if field == 4 {
                self.draft.memory_max = value;
            } else {
                self.draft.memory_min = value;
            }
        }
    }

    fn adjust_selected_memory(&mut self, forward: bool) {
        let value = adjust_memory(&self.effective_memory(self.selected), forward);
        self.set_memory(self.selected, Some(value));
        self.error = None;
    }

    fn resolution_choices(&self) -> Vec<ResolutionChoice> {
        resolution_choices(self.draft.resolution, &self.display_resolutions)
    }

    fn default_resolution(&self) -> Option<(u32, u32)> {
        self.display_resolutions
            .first()
            .map(|display| (display.width, display.height))
    }

    fn apply_default_memory(&mut self) {
        let settings = SETTINGS.read();
        if self.selected == 4 {
            self.draft.memory_min = Some(settings.defaults.memory_min.clone());
        } else {
            self.draft.memory_max = Some(settings.defaults.memory_max.clone());
        }
        self.error = None;
    }

    fn apply_default_resolution(&mut self) {
        if let Some(resolution) = self.default_resolution() {
            self.draft.resolution = Some(resolution);
            self.error = None;
        } else {
            self.error = Some("Display resolution could not be detected.".to_owned());
        }
    }

    fn toggle_auto_java(&mut self) {
        self.draft.java_path = if self.draft.java_path.is_none() {
            Some(self.java_picker.detected_path().to_owned())
        } else {
            None
        };
        self.error = None;
    }

    fn close_version_picker(&mut self) {
        let cancel_runtime_change = self.picker == Some(VersionPicker::Loader)
            && self.runtime_changed()
            && self.draft.loader_version.is_none();
        self.picker = None;
        self.picker_search.deactivate();
        if cancel_runtime_change {
            self.cancel_runtime_change();
        }
    }

    fn handle_picker_key(&mut self, key: &KeyEvent) {
        self.initialize_picker_index();
        if self.picker_search.active {
            match key.code {
                KeyCode::Esc => {
                    self.picker_search.deactivate();
                    return;
                }
                KeyCode::Backspace => {
                    self.picker_search.backspace(key.modifiers);
                    self.picker_index = 0;
                    return;
                }
                KeyCode::Char('j') | KeyCode::Down | KeyCode::Char('k') | KeyCode::Up => {}
                KeyCode::Char(character) => {
                    self.picker_search.push(character);
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
            KeyCode::Esc => self.close_version_picker(),
            KeyCode::Char('h') | KeyCode::Left if !self.picker_search.active => {
                self.close_version_picker();
            }
            KeyCode::Char('/') if !self.picker_search.active => {
                self.picker_search.activate();
                self.picker_index = 0;
            }
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
            KeyCode::Enter if self.picker_search.active => self.picker_search.confirm(),
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => match self.picker {
                Some(VersionPicker::Game) => {
                    if let Some(version) = self.visible_game_versions().get(self.picker_index) {
                        if self.draft.game_version != version.id {
                            self.draft.game_version = version.id.clone();
                            self.draft.loader_version = None;
                            self.loader_versions = Arc::new(Mutex::new(LoadState::Idle));
                            self.picker = None;
                            if self.draft.loader != ModLoader::Vanilla {
                                self.open_loader_picker();
                            }
                        } else {
                            self.picker = None;
                        }
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
            0 if value.is_empty() => invalid(self, "Game version is required.".to_owned()),
            0 => self.draft.game_version = value.to_owned(),
            2 => self.draft.loader_version = (!value.is_empty()).then(|| value.to_owned()),
            3 => self.draft.java_path = (!value.is_empty()).then(|| value.to_owned()),
            4 | 5 if !value.is_empty() && normalize_memory_value(value).is_none() => invalid(
                self,
                "Use a positive memory value ending in K, M, or G.".to_owned(),
            ),
            4 => self.set_memory(4, normalize_memory_value(value)),
            5 => self.set_memory(5, normalize_memory_value(value)),
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
        let before = self.draft.clone();
        let desktop_before = self.desktop;
        let action = self.handle_key_inner(key);
        if !matches!(action, Action::None) {
            return action;
        }
        if let Some(error) = self.error.take() {
            return Action::Error(error);
        }
        if before == self.draft && desktop_before == self.desktop || !self.dirty() {
            return Action::None;
        }
        if self.runtime_changed() {
            if self.draft.loader != ModLoader::Vanilla
                && self
                    .draft
                    .loader_version
                    .as_deref()
                    .is_none_or(str::is_empty)
            {
                return Action::None;
            }
            if !self.validate_before_save() {
                return Action::Error(
                    self.error
                        .take()
                        .unwrap_or_else(|| "invalid instance settings".to_owned()),
                );
            }
            return Action::ConfirmRuntime {
                name: self.draft.name.clone(),
            };
        }
        if self.validate_before_save() {
            Action::Save(Box::new(self.draft.clone()), self.desktop)
        } else {
            Action::Error(
                self.error
                    .take()
                    .unwrap_or_else(|| "invalid instance settings".to_owned()),
            )
        }
    }

    fn handle_key_inner(&mut self, key: &KeyEvent) -> Action {
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
                _ => handle_text_area_input(input, key),
            }
            return Action::None;
        }

        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.selected = (self.selected + 1).min(FIELD_COUNT - 1)
            }
            KeyCode::Char('k') | KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Char('h') | KeyCode::Left if matches!(self.selected, 4 | 5) => {
                self.adjust_selected_memory(false);
            }
            KeyCode::Char('l') | KeyCode::Right if matches!(self.selected, 4 | 5) => {
                self.adjust_selected_memory(true);
            }
            KeyCode::Enter => self.begin_edit(),
            KeyCode::Char('d') if matches!(self.selected, 4 | 5) => {
                self.apply_default_memory();
            }
            KeyCode::Char('d') if self.selected == 7 => self.apply_default_resolution(),
            KeyCode::Char('c') if self.selected == 7 => {
                self.editing = Some(new_text_area(vec![self.value(7)]));
            }
            KeyCode::Char('a') if self.selected == 3 => self.toggle_auto_java(),
            KeyCode::Char('c') if self.selected == 3 => {
                self.editing = Some(new_text_area(vec![self.value(3)]));
            }
            KeyCode::Char('E') => return Action::OpenRaw,
            KeyCode::Esc => return Action::Close,
            _ => {}
        }
        Action::None
    }

    pub fn confirmed_save(&mut self) -> Option<(Box<InstanceConfig>, bool)> {
        self.validate_before_save()
            .then(|| (Box::new(self.draft.clone()), self.desktop))
    }

    pub fn mark_saved(&mut self, saved: &InstanceConfig, desktop: bool) {
        self.original = saved.clone();
        self.draft = saved.clone();
        self.original_desktop = desktop;
        self.desktop = desktop;
        self.error = None;
    }

    pub fn cancel_runtime_change(&mut self) {
        self.draft.game_version = self.original.game_version.clone();
        self.draft.loader = self.original.loader;
        self.draft.loader_version = self.original.loader_version.clone();
        self.game_versions = Arc::new(Mutex::new(LoadState::Idle));
        self.loader_versions = Arc::new(Mutex::new(LoadState::Idle));
        self.picker_initialized = false;
        self.error = None;
    }
}

fn resolution_choices(
    current: Option<(u32, u32)>,
    displays: &[DisplayResolution],
) -> Vec<ResolutionChoice> {
    let mut choices = Vec::new();
    choices.extend(displays.iter().cloned().map(ResolutionChoice::Display));
    for (width, height) in [
        (854, 480),
        (1280, 720),
        (1600, 900),
        (1920, 1080),
        (2560, 1440),
        (3840, 2160),
    ] {
        if !choices
            .iter()
            .any(|choice| choice.resolution() == Some((width, height)))
        {
            choices.push(ResolutionChoice::Preset(width, height));
        }
    }
    if let Some((width, height)) = current
        && !choices
            .iter()
            .any(|choice| choice.resolution() == Some((width, height)))
    {
        choices.push(ResolutionChoice::Configured(width, height));
    }
    choices
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
    let height = if state.picker.is_some() || state.choice_picker == Some(ChoicePicker::Java) {
        (area.height * 2 / 3).max(10)
    } else if state.choice_picker.is_some() {
        10
    } else {
        let form_width = (area.width * 58 / 100).saturating_sub(2);
        11 + jvm_row_count(state, form_width).saturating_sub(1) as u16
    };
    let width = match state.choice_picker {
        Some(ChoicePicker::Java) => 72,
        Some(ChoicePicker::Resolution) => 64,
        _ => 58,
    };
    area.centered(
        Constraint::Percentage(width),
        Constraint::Length(height.min(area.height.saturating_sub(4))),
    )
}

pub fn render(frame: &mut Frame, area: Rect, state: &mut State) {
    if state.choice_picker == Some(ChoicePicker::Java) {
        state.java_picker.initialize();
        state.choice_index = state.java_picker.selected;
    }
    let theme = THEME.as_ref();
    frame.render_widget(Clear, area);
    let title = match (state.picker, state.choice_picker) {
        (Some(VersionPicker::Game), _) => " Minecraft Version ",
        (Some(VersionPicker::Loader), _) => " Loader Version ",
        (_, Some(ChoicePicker::Loader)) => " Mod Loader ",
        (_, Some(ChoicePicker::Java)) => " Java Runtime ",
        (_, Some(ChoicePicker::Resolution)) => " Resolution ",
        _ => " Instance Settings ",
    };
    let keybinds = if state.editing.is_some() {
        super::keybind_line(&[("Enter", " apply"), ("Esc", " cancel")])
    } else if state.picker == Some(VersionPicker::Game) {
        super::keybind_line(&[
            ("/", " search"),
            ("s", " snap"),
            ("h", " back"),
            ("Enter", " select"),
        ])
    } else if state.picker.is_some() {
        super::keybind_line(&[("/", " search"), ("h", " back"), ("Enter", " select")])
    } else if state.choice_picker == Some(ChoicePicker::Java) {
        super::keybind_line(&[("a", " auto"), ("h", " back"), ("Enter", " select")])
    } else if state.choice_picker == Some(ChoicePicker::Resolution) {
        super::keybind_line(&[("d", " default"), ("h", " back"), ("Enter", " select")])
    } else if state.choice_picker.is_some() {
        super::keybind_line(&[("h", " back"), ("Enter", " select")])
    } else if matches!(state.selected, 4 | 5) {
        super::keybind_line(&[
            ("h/l", " adjust"),
            ("Enter", " exact"),
            ("d", " default"),
            ("Esc", " back"),
        ])
    } else if state.selected == 3 {
        super::keybind_line(&[
            ("Enter", " runtimes"),
            ("a", " auto"),
            ("c", " custom"),
            ("Esc", " back"),
        ])
    } else if state.selected == 7 {
        super::keybind_line(&[
            ("Enter", " presets"),
            ("c", " custom"),
            ("d", " default"),
            ("Esc", " back"),
        ])
    } else if matches!(state.selected, 0..=2) {
        super::keybind_line(&[
            ("j/k", ""),
            ("Enter", " select"),
            ("E", " raw"),
            ("Esc", " back"),
        ])
    } else if state.selected == 8 {
        super::keybind_line(&[
            ("j/k", ""),
            ("Enter", " toggle"),
            ("E", " raw"),
            ("Esc", " back"),
        ])
    } else {
        super::keybind_line(&[
            ("j/k", ""),
            ("Enter", " edit"),
            ("E", " raw"),
            ("Esc", " back"),
        ])
    };
    let mut block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BORDER_STYLE.to_border_type())
        .border_style(Style::default().fg(theme.text_dim()))
        .style(Style::default().bg(theme.surface()))
        .title_bottom(keybinds.right_aligned());
    if state.picker.is_some()
        && let Some(search) = state.picker_search.title_line()
    {
        block = block.title_top(search);
    }
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
    let mut y = area.y;
    for (index, label) in labels.iter().enumerate() {
        let lines = if index == 6 {
            jvm_field_lines(state, area.width)
        } else {
            vec![field_line(state, index, label)]
        };
        let height = lines.len() as u16;
        let row_area = Rect { y, height, ..area };
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().bg(if state.selected == index {
                theme.stripe()
            } else {
                theme.surface()
            })),
            row_area,
        );

        if matches!(index, 4 | 5) {
            let value = state.effective_memory(index);
            render_memory_gauge(
                frame,
                Rect {
                    x: area.x.saturating_add(20),
                    y,
                    width: area.width.saturating_sub(21),
                    height: 1,
                },
                &value,
                state.display_value(index),
                state.selected == index,
            );
        }
        if state.selected == index
            && let Some(editor) = state.editing.as_ref()
        {
            frame.render_widget(
                editor,
                Rect {
                    x: area.x.saturating_add(20),
                    y,
                    width: area.width.saturating_sub(20),
                    height: 1,
                },
            );
        }
        y = y.saturating_add(height);
    }
}

fn field_line(state: &State, index: usize, label: &str) -> Line<'static> {
    let theme = THEME.as_ref();
    let selected = index == state.selected;
    let editing = selected && state.editing.is_some();
    let value = if editing || matches!(index, 4..=6) {
        String::new()
    } else {
        state.display_value(index)
    };
    let mut spans = vec![
        Span::styled(
            if selected { "▌ " } else { "  " },
            Style::default().fg(theme.accent()),
        ),
        Span::styled(
            format!("{label:<18}"),
            Style::default().fg(theme.text_dim()),
        ),
        Span::styled(
            value,
            Style::default()
                .fg(if index == 8 {
                    if state.desktop {
                        theme.text()
                    } else {
                        theme.text_dim()
                    }
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
    ];
    if index == 3 && state.draft.java_path.is_none() && !editing {
        spans.extend([Span::raw("  "), auto_label()]);
    }
    Line::from(spans)
}

fn jvm_field_lines(state: &State, width: u16) -> Vec<Line<'static>> {
    let theme = THEME.as_ref();
    let selected = state.selected == 6;
    let mut prefix = field_line(state, 6, "JVM args").spans;
    if selected && state.editing.is_some() {
        return vec![Line::from(prefix)];
    }
    if state.draft.jvm_args.is_empty() {
        prefix.push(Span::styled(
            "no arguments",
            Style::default().fg(theme.text_dim()),
        ));
        return vec![Line::from(prefix)];
    }

    let available = width.saturating_sub(20) as usize;
    let mut lines = Vec::new();
    let mut spans = prefix;
    let mut used = 0usize;
    for argument in &state.draft.jvm_args {
        let badge_width = argument.chars().count() + 2;
        let separator = usize::from(used > 0);
        if used > 0 && used + separator + badge_width > available {
            lines.push(Line::from(spans));
            spans = vec![Span::raw(" ".repeat(20))];
            used = 0;
        }
        if used > 0 {
            spans.push(Span::raw(" "));
            used += 1;
        }
        spans.push(Span::styled(
            format!(" {argument} "),
            Style::default()
                .fg(if selected {
                    theme.accent()
                } else {
                    theme.text()
                })
                .bg(theme.background())
                .add_modifier(Modifier::BOLD),
        ));
        used += badge_width;
    }
    lines.push(Line::from(spans));
    lines
}

fn jvm_row_count(state: &State, width: u16) -> usize {
    if state.draft.jvm_args.is_empty() || state.selected == 6 && state.editing.is_some() {
        return 1;
    }
    let available = width.saturating_sub(20) as usize;
    let mut rows = 1;
    let mut used = 0usize;
    for argument in &state.draft.jvm_args {
        let badge_width = argument.chars().count() + 2;
        let separator = usize::from(used > 0);
        if used > 0 && used + separator + badge_width > available {
            rows += 1;
            used = 0;
        }
        used += usize::from(used > 0) + badge_width;
    }
    rows
}

fn render_choice_picker(frame: &mut Frame, area: Rect, state: &State) {
    let theme = THEME.as_ref();
    let mut list_area = area;
    if state.choice_picker == Some(ChoicePicker::Java)
        && let Some(status) = state.java_picker.status()
    {
        let (message, color) = match status {
            Ok(message) => (message.to_owned(), theme.text_dim()),
            Err(error) => (error, theme.error()),
        };
        frame.render_widget(
            Paragraph::new(message).style(Style::default().fg(color)),
            Rect { height: 1, ..area },
        );
        list_area.y = list_area.y.saturating_add(1);
        list_area.height = list_area.height.saturating_sub(1);
    }
    let items = match state.choice_picker {
        Some(ChoicePicker::Java) => state.java_picker.items(),
        Some(ChoicePicker::Resolution) => resolution_items(&state.resolution_choices()),
        _ => state
            .choice_values()
            .iter()
            .map(|value| {
                ListItem::new(Line::from(Span::styled(
                    value.clone(),
                    Style::default().fg(theme.text()),
                )))
            })
            .collect(),
    };
    super::select_list::render(items, state.choice_index, list_area, frame.buffer_mut());
}

fn resolution_items(choices: &[ResolutionChoice]) -> Vec<ListItem<'static>> {
    let theme = THEME.as_ref();
    choices
        .iter()
        .map(|choice| {
            let mut spans = vec![Span::styled(
                choice.label(),
                Style::default().fg(theme.text()),
            )];
            match choice {
                ResolutionChoice::Preset(_, _) => {}
                ResolutionChoice::Display(display) => {
                    if !display.name.is_empty() {
                        spans.extend([
                            Span::raw("  "),
                            Span::styled(
                                format!(" {} ", display.name),
                                Style::default()
                                    .fg(if display.primary {
                                        theme.success()
                                    } else {
                                        theme.info()
                                    })
                                    .bg(theme.stripe()),
                            ),
                        ]);
                    }
                }
                ResolutionChoice::Configured(_, _) => {
                    spans.push(Span::styled(
                        "  configured",
                        Style::default().fg(theme.text_dim()),
                    ));
                }
            }
            ListItem::new(Line::from(spans))
        })
        .collect()
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
    match status {
        PickerLoad::Idle | PickerLoad::Loading => frame.render_widget(
            Paragraph::new("Loading versions...").style(Style::default().fg(theme.text_dim())),
            area,
        ),
        PickerLoad::Error(error) => frame.render_widget(
            Paragraph::new(format!(
                "Failed to load versions: {error}. Reopen to retry."
            ))
            .style(Style::default().fg(theme.error())),
            area,
        ),
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
                Some(VersionPicker::Loader) => {
                    state.visible_loader_versions().into_iter().collect()
                }
                None => Vec::new(),
            };
            if versions.is_empty() {
                frame.render_widget(Paragraph::new("No matching versions."), area);
            } else {
                let items = versions
                    .iter()
                    .map(|version| {
                        ListItem::new(
                            state
                                .picker_search
                                .highlight_line(version, Style::default().fg(theme.text())),
                        )
                    })
                    .collect();
                super::select_list::render(items, state.picker_index, area, frame.buffer_mut());
            }
        }
    }
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
        *state.loader_versions.lock().unwrap() = LoadState::Loaded(vec!["0.16.15".to_owned()]);
        state.picker = Some(VersionPicker::Loader);
        state.picker_initialized = true;
        state.picker_index = 0;

        assert!(matches!(
            state.handle_key(&KeyEvent::from(KeyCode::Enter)),
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
        state.selected = 8;

        let Action::Save(config, _) = state.handle_key(&KeyEvent::from(KeyCode::Enter)) else {
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
    fn loader_change_selects_a_version_then_requests_confirmation() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());
        state.open_choice_picker(ChoicePicker::Loader);
        state.handle_key(&KeyEvent::from(KeyCode::Char('j')));

        assert!(matches!(
            state.handle_key(&KeyEvent::from(KeyCode::Enter)),
            Action::None
        ));
        assert_eq!(state.picker, Some(VersionPicker::Loader));

        *state.loader_versions.lock().unwrap() = LoadState::Loaded(vec!["1.0.0".to_owned()]);
        state.picker_initialized = true;
        state.picker_index = 0;
        assert!(matches!(
            state.handle_key(&KeyEvent::from(KeyCode::Enter)),
            Action::ConfirmRuntime { .. }
        ));
    }

    #[test]
    fn invalid_field_action_is_returned_for_toast_display() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = instance();
        config.loader = ModLoader::Vanilla;
        config.loader_version = None;
        let mut state = State::new(&config, temp.path());
        state.selected = 2;

        assert!(matches!(
            state.handle_key(&KeyEvent::from(KeyCode::Enter)),
            Action::Error(message) if message == "Vanilla does not use a loader version"
        ));
        assert!(state.error.is_none());
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
        assert_eq!(state.picker, Some(VersionPicker::Loader));

        state.picker = None;
        state.selected = 8;
        let desktop = state.desktop;
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert_ne!(state.desktop, desktop);
    }

    #[test]
    fn memory_uses_slider_and_jvm_args_use_inline_editor() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());

        state.selected = 4;
        state.handle_key(&KeyEvent::from(KeyCode::Char('l')));
        assert!(state.draft.memory_min.is_some());
        state.begin_edit();
        assert!(state.editing.is_some());

        state.editing = None;
        state.selected = 6;
        state.begin_edit();
        assert!(state.editing.is_some());
        assert!(state.choice_picker.is_none());
        assert!(state.picker.is_none());
    }

    #[test]
    fn resolution_and_java_use_selectors() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());

        state.selected = 7;
        state.begin_edit();
        assert!(state.editing.is_none());
        assert_eq!(state.choice_picker, Some(ChoicePicker::Resolution));
        state.choice_index = state
            .resolution_choices()
            .iter()
            .position(|choice| choice.resolution() == Some((1920, 1080)))
            .unwrap();
        state.apply_choice();
        assert_eq!(state.draft.resolution, Some((1920, 1080)));

        state.selected = 3;
        state.begin_edit();
        assert_eq!(state.choice_picker, Some(ChoicePicker::Java));
        state.handle_choice_key(&KeyEvent::from(KeyCode::Char('c')));
        assert_eq!(state.choice_picker, Some(ChoicePicker::Java));
        assert!(state.editing.is_none());
        state.handle_choice_key(&KeyEvent::from(KeyCode::Esc));
        state.handle_key(&KeyEvent::from(KeyCode::Char('c')));
        assert!(state.editing.is_some());
    }

    #[test]
    fn java_and_resolution_defaults_are_direct_actions_not_list_rows() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());

        state.selected = 3;
        state.draft.java_path = Some("/custom/java".to_owned());
        state.handle_key(&KeyEvent::from(KeyCode::Char('a')));
        assert_eq!(state.draft.java_path, None);
        state.handle_key(&KeyEvent::from(KeyCode::Char('a')));
        assert_eq!(
            state.draft.java_path.as_deref(),
            Some(state.java_picker.detected_path())
        );

        state.selected = 7;
        state.display_resolutions = vec![DisplayResolution {
            width: 2560,
            height: 1440,
            name: "DP-4".to_owned(),
            primary: true,
        }];
        state.draft.resolution = Some((1920, 1080));
        state.handle_key(&KeyEvent::from(KeyCode::Char('d')));
        assert_eq!(state.draft.resolution, Some((2560, 1440)));
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert!(!state.choice_values().iter().any(|value| value == "Default"));
        assert!(!state.choice_values().iter().any(|value| value == "Custom…"));

        state.handle_key(&KeyEvent::from(KeyCode::Esc));
        state.handle_key(&KeyEvent::from(KeyCode::Char('c')));
        assert!(state.editing.is_some());
    }

    #[test]
    fn memory_default_copies_only_the_selected_launcher_value() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());
        state.draft.memory_min = Some("6G".to_owned());
        state.draft.memory_max = Some("42G".to_owned());
        state.selected = 4;
        let expected = SETTINGS.read().defaults.memory_min.clone();

        state.handle_key(&KeyEvent::from(KeyCode::Char('d')));

        assert_eq!(state.draft.memory_min.as_deref(), Some(expected.as_str()));
        assert_eq!(state.draft.memory_max.as_deref(), Some("42G"));
        assert!(!state.display_value(4).contains("default"));
    }

    #[test]
    fn memory_slider_keeps_bounds_linked_and_desktop_has_no_dot() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());
        state.draft.memory_min = Some("4G".to_owned());
        state.draft.memory_max = Some("4G".to_owned());
        state.selected = 4;

        state.adjust_selected_memory(true);

        assert_eq!(state.draft.memory_min.as_deref(), Some("6G"));
        assert_eq!(state.draft.memory_max.as_deref(), Some("6G"));
        let desktop = state.display_value(8);
        assert!(matches!(desktop.as_str(), "enabled" | "disabled"));
        assert!(!desktop.contains('●') && !desktop.contains('○'));
    }

    #[test]
    fn detected_resolutions_are_listed_before_presets_without_duplicates() {
        let displays = vec![DisplayResolution {
            width: 1920,
            height: 1080,
            name: "Primary display".to_owned(),
            primary: true,
        }];

        let choices = resolution_choices(None, &displays);

        assert!(matches!(choices[0], ResolutionChoice::Display(_)));
        assert_eq!(
            choices
                .iter()
                .filter(|choice| choice.resolution() == Some((1920, 1080)))
                .count(),
            1
        );
    }

    #[test]
    fn inherited_resolution_displays_the_detected_primary_size() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());
        state.display_resolutions = vec![DisplayResolution {
            width: 2560,
            height: 1440,
            name: "DP-4".to_owned(),
            primary: true,
        }];

        assert_eq!(state.display_value(7), "2560x1440");
    }

    #[test]
    fn runtime_changes_do_not_render_an_inline_notification() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());
        state.draft.game_version = "1.21.2".to_owned();
        let backend = ratatui::backend::TestBackend::new(80, 20);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                let area = popup_rect(frame.area(), &state);
                render(frame, area, &mut state);
            })
            .unwrap();

        assert!(!terminal.backend().to_string().contains("installed mods"));
    }

    #[test]
    fn cancelling_chained_loader_version_restores_the_original_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());
        let original = state.original.clone();
        *state.game_versions.lock().unwrap() = LoadState::Loaded(vec![GameVersion {
            id: "1.21.2".to_owned(),
            stable: true,
        }]);
        state.picker = Some(VersionPicker::Game);
        state.picker_initialized = true;

        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert_eq!(state.picker, Some(VersionPicker::Loader));
        assert_eq!(state.draft.loader_version, None);

        state.handle_key(&KeyEvent::from(KeyCode::Esc));
        assert_eq!(state.draft.game_version, original.game_version);
        assert_eq!(state.draft.loader, original.loader);
        assert_eq!(state.draft.loader_version, original.loader_version);
        assert_eq!(state.picker, None);
    }

    #[test]
    fn jvm_argument_badges_wrap_without_hiding_following_fields() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = instance();
        config.jvm_args = [
            "-Xmx4G",
            "-XX:+UseG1GC",
            "-Dexample.first=true",
            "-Dexample.second=true",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let state = State::new(&config, temp.path());

        let lines = jvm_field_lines(&state, 48);

        assert!(lines.len() > 1);
        assert_eq!(jvm_row_count(&state, 48), lines.len());
        assert!(lines.iter().any(|line| line.to_string().contains("second")));
    }
}
