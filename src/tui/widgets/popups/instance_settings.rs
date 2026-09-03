// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// modal editor for settings belonging to the selected Minecraft instance.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
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
    Java,
    Resolution,
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
    jvm_args_open: bool,
    jvm_arg_index: usize,
    jvm_arg_input: Option<(Option<usize>, TextArea<'static>)>,
    pub desktop: bool,
    original_desktop: bool,
    error: Option<String>,
    confirm_close: bool,
    confirm_runtime_change: bool,
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
    detected_java: String,
}

pub enum Action {
    None,
    Save(Box<InstanceConfig>, bool),
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
            jvm_args_open: false,
            jvm_arg_index: 0,
            jvm_arg_input: None,
            desktop: crate::instance::desktop::exists(&instance.name),
            original_desktop: crate::instance::desktop::exists(&instance.name),
            error: None,
            confirm_close: false,
            confirm_runtime_change: false,
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
            detected_java: crate::instance::java::detect_java_path(),
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
            6 => format!("{} argument(s)", self.draft.jvm_args.len()),
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
            3 => self.open_choice_picker(ChoicePicker::Java),
            4 | 5 => self.adjust_memory(self.selected, true),
            6 => {
                self.jvm_args_open = true;
                self.jvm_arg_index = self
                    .jvm_arg_index
                    .min(self.draft.jvm_args.len().saturating_sub(1));
            }
            7 => self.open_choice_picker(ChoicePicker::Resolution),
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
            ChoicePicker::Java => self
                .choice_values_for(picker)
                .iter()
                .position(|value| {
                    self.draft.java_path.as_deref().map_or_else(
                        || value == "automatic / launcher default",
                        |current| value == current,
                    )
                })
                .unwrap_or(0),
            ChoicePicker::Resolution => self
                .choice_values_for(picker)
                .iter()
                .position(|value| {
                    self.draft.resolution.map_or_else(
                        || value == "window default",
                        |(width, height)| value == &format!("{width}x{height}"),
                    )
                })
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
            ChoicePicker::Java => {
                let mut values = vec!["automatic / launcher default".to_owned()];
                if !self.detected_java.is_empty() {
                    values.push(self.detected_java.clone());
                }
                if let Some(current) = &self.draft.java_path
                    && !values.contains(current)
                {
                    values.push(current.clone());
                }
                values.push("custom path…".to_owned());
                values
            }
            ChoicePicker::Resolution => {
                let mut values = vec![
                    "window default".to_owned(),
                    "854x480".to_owned(),
                    "1280x720".to_owned(),
                    "1600x900".to_owned(),
                    "1920x1080".to_owned(),
                    "2560x1440".to_owned(),
                ];
                if let Some((width, height)) = self.draft.resolution {
                    let current = format!("{width}x{height}");
                    if !values.contains(&current) {
                        values.push(current);
                    }
                }
                values.push("custom resolution…".to_owned());
                values
            }
        }
    }

    fn handle_choice_key(&mut self, key: &KeyEvent) {
        let count = self.choice_values().len();
        match key.code {
            KeyCode::Esc => self.choice_picker = None,
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Right if count > 0 => {
                self.choice_index = (self.choice_index + 1).min(count - 1);
            }
            KeyCode::Char('k') | KeyCode::Up | KeyCode::Left => {
                self.choice_index = self.choice_index.saturating_sub(1);
            }
            KeyCode::Enter => self.apply_choice(),
            _ => {}
        }
    }

    fn apply_choice(&mut self) {
        let selected = self
            .choice_values()
            .get(self.choice_index)
            .cloned()
            .unwrap_or_default();
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
            Some(ChoicePicker::Java) if selected == "custom path…" => {
                self.editing = Some(new_text_area(vec![self.value(3)]));
            }
            Some(ChoicePicker::Java) => {
                self.draft.java_path =
                    (selected != "automatic / launcher default").then_some(selected);
            }
            Some(ChoicePicker::Resolution) if selected == "custom resolution…" => {
                self.editing = Some(new_text_area(vec![self.value(7)]));
            }
            Some(ChoicePicker::Resolution) => {
                self.draft.resolution = (selected != "window default")
                    .then(|| parse_resolution(&selected).ok())
                    .flatten();
            }
            None => {}
        }
        self.choice_picker = None;
    }

    fn rotate_choice(&mut self, picker: ChoicePicker, forward: bool) {
        self.open_choice_picker(picker);
        let count = self
            .choice_values()
            .len()
            .saturating_sub(usize::from(matches!(
                picker,
                ChoicePicker::Java | ChoicePicker::Resolution
            )));
        if count > 0 {
            self.choice_index = if forward {
                (self.choice_index + 1) % count
            } else {
                (self.choice_index + count - 1) % count
            };
            self.apply_choice();
        }
    }

    fn adjust_memory(&mut self, field: usize, forward: bool) {
        let settings = SETTINGS.read();
        let current = if field == 4 {
            self.draft
                .memory_min
                .as_deref()
                .unwrap_or(&settings.defaults.memory_min)
        } else {
            self.draft
                .memory_max
                .as_deref()
                .unwrap_or(&settings.defaults.memory_max)
        };
        let values = ["512M", "1G", "2G", "4G", "6G", "8G", "12G", "16G"];
        let exact = values.iter().position(|value| *value == current);
        let next = if let Some(index) = exact {
            if forward {
                (index + 1) % values.len()
            } else {
                (index + values.len() - 1) % values.len()
            }
        } else {
            let current_kib = memory_kib(current).unwrap_or_default();
            if forward {
                values
                    .iter()
                    .position(|value| memory_kib(value).is_some_and(|kib| kib > current_kib))
                    .unwrap_or(0)
            } else {
                values
                    .iter()
                    .rposition(|value| memory_kib(value).is_some_and(|kib| kib < current_kib))
                    .unwrap_or(values.len() - 1)
            }
        };
        drop(settings);
        self.set_memory(field, Some(values[next].to_owned()));
    }

    fn set_memory(&mut self, field: usize, value: Option<String>) {
        if field == 4 {
            self.draft.memory_min = value.clone();
        } else {
            self.draft.memory_max = value.clone();
        }

        let settings = SETTINGS.read();
        let min = self
            .draft
            .memory_min
            .as_deref()
            .unwrap_or(&settings.defaults.memory_min);
        let max = self
            .draft
            .memory_max
            .as_deref()
            .unwrap_or(&settings.defaults.memory_max);
        if memory_kib(min)
            .zip(memory_kib(max))
            .is_some_and(|(min, max)| min > max)
        {
            if field == 4 {
                self.draft.memory_max = value;
            } else {
                self.draft.memory_min = value;
            }
        }
    }

    fn handle_jvm_args_key(&mut self, key: &KeyEvent) {
        if let Some((target, input)) = &mut self.jvm_arg_input {
            match key.code {
                KeyCode::Enter => {
                    let value = input.lines().join(" ").trim().to_owned();
                    let target = *target;
                    self.jvm_arg_input = None;
                    if value.is_empty() {
                        return;
                    }
                    if let Some(index) = target {
                        if let Some(argument) = self.draft.jvm_args.get_mut(index) {
                            *argument = value;
                        }
                    } else {
                        self.draft.jvm_args.push(value);
                        self.jvm_arg_index = self.draft.jvm_args.len() - 1;
                    }
                }
                KeyCode::Esc => self.jvm_arg_input = None,
                _ => {
                    input.input(*key);
                }
            }
            return;
        }

        match key.code {
            KeyCode::Esc => self.jvm_args_open = false,
            KeyCode::Char('j') | KeyCode::Down if !self.draft.jvm_args.is_empty() => {
                self.jvm_arg_index = (self.jvm_arg_index + 1).min(self.draft.jvm_args.len() - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.jvm_arg_index = self.jvm_arg_index.saturating_sub(1);
            }
            KeyCode::Left if self.jvm_arg_index > 0 => {
                self.draft
                    .jvm_args
                    .swap(self.jvm_arg_index, self.jvm_arg_index - 1);
                self.jvm_arg_index -= 1;
            }
            KeyCode::Right if self.jvm_arg_index + 1 < self.draft.jvm_args.len() => {
                self.draft
                    .jvm_args
                    .swap(self.jvm_arg_index, self.jvm_arg_index + 1);
                self.jvm_arg_index += 1;
            }
            KeyCode::Char('a') => {
                self.jvm_arg_input = Some((None, new_text_area(vec![String::new()])));
            }
            KeyCode::Enter if self.draft.jvm_args.is_empty() => {
                self.jvm_arg_input = Some((None, new_text_area(vec![String::new()])));
            }
            KeyCode::Enter if !self.draft.jvm_args.is_empty() => {
                let index = self.jvm_arg_index.min(self.draft.jvm_args.len() - 1);
                self.jvm_arg_input = Some((
                    Some(index),
                    new_text_area(vec![self.draft.jvm_args[index].clone()]),
                ));
            }
            KeyCode::Char('d') if !self.draft.jvm_args.is_empty() => {
                let index = self.jvm_arg_index.min(self.draft.jvm_args.len() - 1);
                self.draft.jvm_args.remove(index);
                self.jvm_arg_index = self
                    .jvm_arg_index
                    .min(self.draft.jvm_args.len().saturating_sub(1));
            }
            _ => {}
        }
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
            KeyCode::Esc => self.picker = None,
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
            KeyCode::Enter => match self.picker {
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
            7 if value.is_empty() => self.draft.resolution = None,
            7 => match parse_resolution(value) {
                Ok(resolution) => self.draft.resolution = Some(resolution),
                Err(error) => invalid(self, error),
            },
            _ => {}
        }
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> Action {
        if self.jvm_args_open {
            self.handle_jvm_args_key(key);
            return Action::None;
        }
        if self.choice_picker.is_some() {
            self.handle_choice_key(key);
            return Action::None;
        }
        if self.picker.is_some() {
            self.handle_picker_key(key);
            return Action::None;
        }
        if self.confirm_close {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => return Action::Close,
                KeyCode::Char('n') | KeyCode::Esc => self.confirm_close = false,
                _ => {}
            }
            return Action::None;
        }
        if self.confirm_runtime_change {
            match key.code {
                KeyCode::Char('y') | KeyCode::Enter => {
                    if self.validate_before_save() {
                        return Action::Save(Box::new(self.draft.clone()), self.desktop);
                    }
                    self.confirm_runtime_change = false;
                }
                KeyCode::Char('n') | KeyCode::Esc => self.confirm_runtime_change = false,
                _ => {}
            }
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
            KeyCode::Left => match self.selected {
                1 => self.rotate_choice(ChoicePicker::Loader, false),
                3 => self.rotate_choice(ChoicePicker::Java, false),
                4 | 5 => self.adjust_memory(self.selected, false),
                7 => self.rotate_choice(ChoicePicker::Resolution, false),
                8 => self.desktop = !self.desktop,
                _ => {}
            },
            KeyCode::Right => match self.selected {
                1 => self.rotate_choice(ChoicePicker::Loader, true),
                3 => self.rotate_choice(ChoicePicker::Java, true),
                4 | 5 => self.adjust_memory(self.selected, true),
                7 => self.rotate_choice(ChoicePicker::Resolution, true),
                8 => self.desktop = !self.desktop,
                _ => {}
            },
            KeyCode::Enter => self.begin_edit(),
            KeyCode::Char('c') if matches!(self.selected, 4 | 5 | 7) => {
                self.editing = Some(new_text_area(vec![self.value(self.selected)]));
            }
            KeyCode::Char('r') if matches!(self.selected, 4 | 5) => {
                self.set_memory(self.selected, None);
            }
            KeyCode::Char('s') if self.dirty() => {
                if self.validate_before_save() {
                    if self.runtime_changed() {
                        self.confirm_runtime_change = true;
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
            KeyCode::Esc if self.dirty() => self.confirm_close = true,
            KeyCode::Esc => return Action::Close,
            _ => {}
        }
        Action::None
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
    let height = if state.picker.is_some() || state.choice_picker.is_some() {
        19
    } else if state.jvm_args_open {
        18
    } else if state.editing.is_some() {
        14
    } else {
        12
    };
    area.centered(
        Constraint::Percentage(76),
        Constraint::Length(height.min(area.height.saturating_sub(4))),
    )
}

pub fn render(frame: &mut Frame, area: Rect, state: &State) {
    let theme = THEME.as_ref();
    frame.render_widget(Clear, area);
    let mut title = vec![
        Span::styled(
            " Instance Settings ",
            Style::default()
                .fg(theme.text())
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("· {} ", state.draft.name),
            Style::default().fg(theme.text_dim()),
        ),
    ];
    if state.dirty() {
        title.push(Span::styled(
            "● modified ",
            Style::default().fg(theme.warning()),
        ));
    }
    let keybinds = if state.jvm_args_open {
        if state.jvm_arg_input.is_some() {
            super::keybind_line(&[("Enter", " apply"), ("Esc", " cancel")])
        } else {
            super::keybind_line(&[
                ("j/k", ""),
                ("←/→", ""),
                ("Enter", " edit"),
                ("a", " add"),
                ("d", " delete"),
                ("Esc", " back"),
            ])
        }
    } else if state.picker.is_some() {
        super::keybind_line(&[
            ("j/k", " move"),
            ("Enter", " select"),
            ("/", " search"),
            ("s", " snapshots"),
            ("Esc", " back"),
        ])
    } else if state.choice_picker.is_some() {
        super::keybind_line(&[("j/k", " move"), ("Enter", " select"), ("Esc", " back")])
    } else if matches!(state.selected, 4 | 5) {
        super::keybind_line(&[
            ("j/k", ""),
            ("←/→", " memory"),
            ("c", " custom"),
            ("r", " default"),
            ("s", ""),
            ("E", ""),
            ("Esc", ""),
        ])
    } else if state.selected == 6 {
        super::keybind_line(&[
            ("j/k", ""),
            ("Enter", " manage args"),
            ("s", " save"),
            ("E", " raw"),
            ("Esc", " back"),
        ])
    } else if state.selected == 8 {
        super::keybind_line(&[
            ("j/k", ""),
            ("Enter", " toggle"),
            ("←/→", " toggle"),
            ("s", " save"),
            ("Esc", " back"),
        ])
    } else {
        super::keybind_line(&[
            ("j/k", ""),
            ("Enter", " open"),
            ("←/→", " switch"),
            ("s", " save"),
            ("E", " raw"),
            ("Esc", " back"),
        ])
    };
    let block = Block::default()
        .title(Line::from(title))
        .borders(Borders::ALL)
        .border_type(BORDER_STYLE.to_border_type())
        .border_style(Style::default().fg(theme.text_dim()))
        .style(Style::default().bg(theme.surface()))
        .title_bottom(keybinds);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.jvm_args_open {
        render_jvm_args(frame, inner, state);
        return;
    }
    if state.picker.is_some() {
        render_version_picker(frame, inner, state);
        return;
    }
    if state.choice_picker.is_some() {
        render_choice_picker(frame, inner, state);
        return;
    }

    render_settings_form(frame, inner, state);
}

fn render_settings_form(frame: &mut Frame, area: Rect, state: &State) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);
    let top = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(sections[0]);

    render_field_card(
        frame,
        top[0],
        " Runtime ",
        state,
        &[
            (0, "Version"),
            (1, "Loader"),
            (2, "Loader ver."),
            (3, "Java"),
        ],
    );
    render_field_card(
        frame,
        top[1],
        " Launch ",
        state,
        &[
            (4, "Min memory"),
            (5, "Max memory"),
            (6, "JVM args"),
            (7, "Resolution"),
        ],
    );

    let desktop_block = settings_card(" Desktop ", state.selected == 8);
    let desktop_inner = desktop_block.inner(sections[1]);
    frame.render_widget(desktop_block, sections[1]);
    frame.render_widget(
        Paragraph::new(field_line(state, 8, "Shortcut", 12)),
        desktop_inner,
    );

    if let Some(editor) = state.editing.as_ref() {
        let theme = THEME.as_ref();
        let editor_block = Block::default()
            .title(match state.selected {
                3 => " Custom Java executable path · Enter apply ",
                4 => " Custom minimum memory (K/M/G) · Enter apply ",
                5 => " Custom maximum memory (K/M/G) · Enter apply ",
                7 => " Custom resolution (WIDTHxHEIGHT) · Enter apply ",
                _ => " Value · Enter apply ",
            })
            .borders(Borders::ALL)
            .border_type(BORDER_STYLE.to_border_type())
            .border_style(Style::default().fg(theme.accent()))
            .style(Style::default().bg(theme.surface()));
        let editor_inner = editor_block.inner(sections[2]);
        frame.render_widget(editor_block, sections[2]);
        frame.render_widget(editor, editor_inner);
    } else {
        frame.render_widget(
            Paragraph::new(status_line(state)).wrap(Wrap { trim: true }),
            sections[2],
        );
    }
}

fn settings_card(title: &'static str, active: bool) -> Block<'static> {
    let theme = THEME.as_ref();
    Block::default()
        .title(Span::styled(
            title,
            Style::default().fg(if active {
                theme.accent()
            } else {
                theme.text_dim()
            }),
        ))
        .borders(Borders::ALL)
        .border_type(BORDER_STYLE.to_border_type())
        .border_style(Style::default().fg(if active {
            theme.accent()
        } else {
            theme.border()
        }))
        .style(Style::default().bg(theme.surface()))
}

fn render_field_card(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    state: &State,
    fields: &[(usize, &str)],
) {
    let active = fields.iter().any(|(index, _)| *index == state.selected);
    let block = settings_card(title, active);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = fields
        .iter()
        .map(|(index, label)| field_line(state, *index, label, 12))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn field_line<'a>(state: &'a State, index: usize, label: &str, label_width: usize) -> Line<'a> {
    let theme = THEME.as_ref();
    let selected = index == state.selected;
    let displayed = state.display_value(index);
    let value_color = if index == 8 && state.desktop {
        theme.success()
    } else if selected {
        theme.accent()
    } else {
        theme.text()
    };
    let mut spans = vec![
        Span::styled(
            if selected { "▶ " } else { "  " },
            Style::default().fg(theme.accent()),
        ),
        Span::styled(
            format!("{label:<label_width$}"),
            Style::default().fg(theme.text_dim()),
        ),
    ];
    if matches!(index, 4 | 5) {
        spans.push(memory_slider(state, index, selected));
    } else if matches!(index, 1 | 3 | 7 | 8) && state.editing.is_none() {
        spans.push(Span::styled(
            format!(" ‹ {displayed} › "),
            Style::default()
                .fg(value_color)
                .bg(theme.background())
                .add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
    } else {
        spans.push(Span::styled(
            displayed,
            Style::default().fg(value_color).add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        ));
    }
    Line::from(spans).style(Style::default().bg(if selected {
        theme.stripe()
    } else {
        theme.surface()
    }))
}

fn memory_slider(state: &State, index: usize, selected: bool) -> Span<'static> {
    let theme = THEME.as_ref();
    let settings = SETTINGS.read();
    let configured = if index == 4 {
        state.draft.memory_min.as_deref()
    } else {
        state.draft.memory_max.as_deref()
    };
    let effective = configured.unwrap_or_else(|| {
        if index == 4 {
            &settings.defaults.memory_min
        } else {
            &settings.defaults.memory_max
        }
    });
    let thresholds = ["512M", "1G", "2G", "4G", "6G", "8G", "12G", "16G"];
    let effective_kib = memory_kib(effective).unwrap_or(0);
    let step = thresholds
        .iter()
        .position(|value| memory_kib(value).is_some_and(|limit| effective_kib <= limit))
        .unwrap_or(thresholds.len() - 1);
    let filled = ((step + 1) * 6).div_ceil(thresholds.len()).max(1);
    let bar = format!("{}{}", "▰".repeat(filled), "▱".repeat(6 - filled));
    let label = if configured.is_none() {
        format!("default {effective}")
    } else {
        effective.to_owned()
    };
    Span::styled(
        format!(" ◀ {bar} {label} ▶ "),
        Style::default()
            .fg(if selected {
                theme.accent()
            } else {
                theme.text()
            })
            .bg(theme.background())
            .add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
    )
}

fn render_jvm_args(frame: &mut Frame, area: Rect, state: &State) {
    let theme = THEME.as_ref();
    let block = settings_card(" JVM Arguments ", true);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some((target, editor)) = &state.jvm_arg_input {
        let editor_block = Block::default()
            .title(if target.is_some() {
                " Edit argument · Enter apply "
            } else {
                " Add argument · Enter apply "
            })
            .borders(Borders::ALL)
            .border_type(BORDER_STYLE.to_border_type())
            .border_style(Style::default().fg(theme.accent()))
            .style(Style::default().bg(theme.surface()));
        let editor_inner = editor_block.inner(inner);
        frame.render_widget(editor_block, inner);
        frame.render_widget(editor, editor_inner);
        return;
    }

    if state.draft.jvm_args.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  No custom JVM arguments.",
                    Style::default().fg(theme.text_dim()),
                )),
                Line::from(vec![
                    Span::styled(
                        "  [a] ",
                        Style::default()
                            .fg(theme.accent())
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("add an argument", Style::default().fg(theme.text())),
                ]),
            ]),
            inner,
        );
        return;
    }

    let visible_rows = inner.height as usize;
    let start = state
        .jvm_arg_index
        .saturating_sub(visible_rows.saturating_sub(1));
    let lines = state
        .draft
        .jvm_args
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_rows)
        .map(|(index, argument)| {
            let selected = index == state.jvm_arg_index;
            Line::from(vec![
                Span::styled(
                    if selected { "▶ " } else { "  " },
                    Style::default().fg(theme.accent()),
                ),
                Span::styled(
                    format!("{:02}  ", index + 1),
                    Style::default().fg(theme.text_dim()),
                ),
                Span::styled(
                    argument.clone(),
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
            ])
            .style(Style::default().bg(if selected {
                theme.stripe()
            } else {
                theme.surface()
            }))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn status_line(state: &State) -> Line<'_> {
    let theme = THEME.as_ref();
    if let Some(error) = &state.error {
        Line::from(Span::styled(
            format!("  × {error}"),
            Style::default().fg(theme.error()),
        ))
    } else if state.confirm_runtime_change {
        Line::from(Span::styled(
            "  ! Runtime changes download files and may break mods.  [y] continue  [n] cancel",
            Style::default().fg(theme.warning()),
        ))
    } else if state.confirm_close {
        Line::from(Span::styled(
            "  ! Discard unsaved changes?  [y] yes  [n] no",
            Style::default().fg(theme.warning()),
        ))
    } else {
        let settings = SETTINGS.read();
        Line::from(vec![
            Span::styled("  ◇ Defaults  ", Style::default().fg(theme.info())),
            Span::styled(
                format!(
                    "empty Java or memory values inherit {} → {}",
                    settings.defaults.memory_min, settings.defaults.memory_max
                ),
                Style::default().fg(theme.text_dim()),
            ),
        ])
    }
}

fn render_choice_picker(frame: &mut Frame, area: Rect, state: &State) {
    let theme = THEME.as_ref();
    let title = match state.choice_picker {
        Some(ChoicePicker::Loader) => " Loader ",
        Some(ChoicePicker::Java) => " Java Runtime ",
        Some(ChoicePicker::Resolution) => " Window Resolution ",
        None => return,
    };
    let block = settings_card(title, true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines = Vec::new();
    let values = state.choice_values();
    let visible_rows = inner.height as usize;
    let start = state
        .choice_index
        .saturating_sub(visible_rows.saturating_sub(1));
    for (index, value) in values.iter().enumerate().skip(start).take(visible_rows) {
        let selected = index == state.choice_index;
        lines.push(
            Line::from(vec![
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
            ])
            .style(Style::default().bg(if selected {
                theme.stripe()
            } else {
                theme.surface()
            })),
        );
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_version_picker(frame: &mut Frame, area: Rect, state: &State) {
    let theme = THEME.as_ref();
    let title = match state.picker {
        Some(VersionPicker::Game) => " Minecraft Version ",
        Some(VersionPicker::Loader) => " Loader Version ",
        None => return,
    };
    let block = settings_card(title, true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
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
                let visible_rows = inner.height.saturating_sub(1) as usize;
                let start = state
                    .picker_index
                    .saturating_sub(visible_rows.saturating_sub(1));
                for (index, version) in versions.iter().enumerate().skip(start).take(visible_rows) {
                    let selected = index == state.picker_index;
                    lines.push(
                        Line::from(vec![
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
                        ])
                        .style(Style::default().bg(if selected {
                            theme.stripe()
                        } else {
                            theme.surface()
                        })),
                    );
                }
            }
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
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
    fn custom_memory_editor_and_jvm_argument_manager_preserve_values() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = instance();
        config.memory_min = Some("512M".to_owned());
        let mut state = State::new(&config, temp.path());
        state.selected = 4;
        state.handle_key(&KeyEvent::from(KeyCode::Char('c')));
        state.handle_key(&KeyEvent::from(KeyCode::Left));
        state.handle_key(&KeyEvent::from(KeyCode::Char('0')));
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert_eq!(state.draft.memory_min.as_deref(), Some("5120M"));

        state.selected = 6;
        state.begin_edit();
        state.handle_key(&KeyEvent::from(KeyCode::Char('a')));
        for character in "-Xfoo".chars() {
            state.handle_key(&KeyEvent::from(KeyCode::Char(character)));
        }
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        state.handle_key(&KeyEvent::from(KeyCode::Char('a')));
        for character in "-Xbar".chars() {
            state.handle_key(&KeyEvent::from(KeyCode::Char(character)));
        }
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert_eq!(state.draft.jvm_args, ["-Xfoo", "-Xbar"]);

        state.handle_key(&KeyEvent::from(KeyCode::Left));
        assert_eq!(state.draft.jvm_args, ["-Xbar", "-Xfoo"]);
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        state.handle_key(&KeyEvent::from(KeyCode::Char('2')));
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert_eq!(state.draft.jvm_args, ["-Xbar2", "-Xfoo"]);
        state.handle_key(&KeyEvent::from(KeyCode::Char('d')));
        assert_eq!(state.draft.jvm_args, ["-Xfoo"]);
    }

    #[test]
    fn runtime_changes_require_confirmation_before_save() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());
        state.draft.game_version = "1.21.2".to_owned();

        assert!(matches!(
            state.handle_key(&KeyEvent::from(KeyCode::Char('s'))),
            Action::None
        ));
        assert!(state.confirm_runtime_change);
        assert!(matches!(
            state.handle_key(&KeyEvent::from(KeyCode::Char('y'))),
            Action::Save(_, _)
        ));
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
    fn arrow_keys_rotate_badged_choices() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());

        state.selected = 1;
        state.handle_key(&KeyEvent::from(KeyCode::Right));
        assert_eq!(state.draft.loader, ModLoader::Forge);
        state.handle_key(&KeyEvent::from(KeyCode::Left));
        assert_eq!(state.draft.loader, ModLoader::Fabric);

        state.selected = 8;
        let desktop = state.desktop;
        state.handle_key(&KeyEvent::from(KeyCode::Right));
        assert_ne!(state.desktop, desktop);
    }

    #[test]
    fn memory_slider_and_resolution_picker_use_structured_values() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = State::new(&instance(), temp.path());

        state.selected = 4;
        state.begin_edit();
        assert!(state.draft.memory_min.is_some());
        assert!(state.choice_picker.is_none());
        assert!(state.editing.is_none());

        state.draft.memory_min = Some("4G".to_owned());
        state.draft.memory_max = Some("4G".to_owned());
        state.handle_key(&KeyEvent::from(KeyCode::Right));
        assert_eq!(state.draft.memory_min.as_deref(), Some("6G"));
        assert_eq!(state.draft.memory_max.as_deref(), Some("6G"));

        state.selected = 7;
        state.begin_edit();
        state.choice_index = state
            .choice_values()
            .iter()
            .position(|value| value == "1920x1080")
            .unwrap();
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert_eq!(state.draft.resolution, Some((1920, 1080)));
        assert!(state.editing.is_none());
    }
}
