// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// modal editor for launcher-wide settings.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, ListItem, Paragraph},
};
use ratatui_textarea::{CursorMove, TextArea};

use crate::{
    config::{
        Config,
        theme::{BORDER_STYLE, BorderStyle, THEME, ThemeConfig},
    },
    instance::models::normalize_memory_value,
    tui::widgets::popups::settings_controls::{
        JavaChoice, JavaPicker, adjust_memory, auto_label, handle_text_area_input, memory_kib,
        render_memory_gauge,
    },
};

pub struct State {
    pub config: Config,
    pub theme: ThemeConfig,
    selected: usize,
    editing: Option<TextArea<'static>>,
    error: Option<String>,
    save_pending: bool,
    themes: Vec<String>,
    theme_picker: bool,
    theme_index: usize,
    java_picker_open: bool,
    java_picker: JavaPicker,
}

pub enum Action {
    None,
    Save(Box<Config>, String, BorderStyle),
    Error(String),
    OpenRaw(std::path::PathBuf),
    Close,
}

impl State {
    pub fn new() -> Self {
        let theme = crate::config::theme::current_theme_config();
        let themes = available_themes();
        let theme_index = themes
            .iter()
            .position(|candidate| candidate == &theme.theme)
            .unwrap_or(0);
        Self {
            config: crate::config::SETTINGS.read().clone(),
            theme,
            selected: 0,
            editing: None,
            error: None,
            save_pending: false,
            themes,
            theme_picker: false,
            theme_index,
            java_picker_open: false,
            java_picker: JavaPicker::new(),
        }
    }

    fn value(&self, field: usize) -> String {
        match field {
            0 => self.theme.theme.clone(),
            1 => format!("{:?}", self.theme.border_style).to_lowercase(),
            2 => self.config.defaults.memory_min.clone(),
            3 => self.config.defaults.memory_max.clone(),
            4 => self.config.paths.java_path.clone().unwrap_or_default(),
            _ => String::new(),
        }
    }

    fn display_value(&self, field: usize) -> String {
        match field {
            1 => format!("{:?}", self.theme.border_style).to_lowercase(),
            4 if self
                .config
                .paths
                .java_path
                .as_deref()
                .is_none_or(str::is_empty) =>
            {
                self.java_picker.detected_path().to_owned()
            }
            _ => self.value(field),
        }
    }

    fn commit_edit(&mut self) {
        let Some(editor) = self.editing.take() else {
            return;
        };
        let raw = editor.lines().join("");
        let value = raw.trim();
        self.error = None;
        let invalid = |state: &mut Self, message: String| {
            state.error = Some(message);
            state.editing = Some(new_text_area(editor.lines().to_vec()));
        };
        match self.selected {
            2 | 3 if normalize_memory_value(value).is_none() => invalid(
                self,
                "Use a positive memory value ending in K, M, or G.".to_owned(),
            ),
            2 => {
                let value = normalize_memory_value(value).unwrap();
                self.config.defaults.memory_min = value.clone();
                if memory_kib(&value) > memory_kib(&self.config.defaults.memory_max) {
                    self.config.defaults.memory_max = value;
                }
                self.save_pending = true;
            }
            3 => {
                let value = normalize_memory_value(value).unwrap();
                self.config.defaults.memory_max = value.clone();
                if memory_kib(&value) < memory_kib(&self.config.defaults.memory_min) {
                    self.config.defaults.memory_min = value;
                }
                self.save_pending = true;
            }
            4 => {
                self.config.paths.java_path = (!value.is_empty()).then(|| value.to_owned());
                self.save_pending = true;
            }
            _ => {}
        }
    }

    fn select_theme(&mut self) {
        let previous = self.theme.theme.clone();
        self.theme.theme = self.themes[self.theme_index].clone();
        self.error = None;
        if let Err(error) = crate::config::theme::apply_theme(
            self.theme.theme.clone(),
            self.theme.border_style.clone(),
        ) {
            self.error = Some(error.to_string());
            self.theme.theme = previous;
        } else if self.theme.theme != previous {
            self.save_pending = true;
        }
    }

    fn handle_theme_picker_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => self.theme_picker = false,
            KeyCode::Char('j') | KeyCode::Down => {
                self.theme_index = (self.theme_index + 1).min(self.themes.len() - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.theme_index = self.theme_index.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                self.select_theme();
                if self.error.is_none() {
                    self.theme_picker = false;
                }
            }
            _ => {}
        }
    }

    fn cycle_border(&mut self, forward: bool) {
        let previous = self.theme.border_style.clone();
        self.theme.border_style = match (&self.theme.border_style, forward) {
            (BorderStyle::Rounded, true) | (BorderStyle::Double, false) => BorderStyle::Plain,
            (BorderStyle::Plain, true) | (BorderStyle::Thick, false) => BorderStyle::Double,
            (BorderStyle::Double, true) | (BorderStyle::Rounded, false) => BorderStyle::Thick,
            (BorderStyle::Thick, true) | (BorderStyle::Plain, false) => BorderStyle::Rounded,
        };
        self.error = None;
        if let Err(error) = crate::config::theme::apply_theme(
            self.theme.theme.clone(),
            self.theme.border_style.clone(),
        ) {
            self.error = Some(error.to_string());
            self.theme.border_style = previous;
        } else if self.theme.border_style != previous {
            self.save_pending = true;
        }
    }

    fn open_java_picker(&mut self) {
        self.java_picker
            .open(self.config.paths.java_path.as_deref());
        self.java_picker.initialize();
        self.java_picker_open = true;
    }

    fn toggle_auto_java(&mut self) {
        self.config.paths.java_path = if self.config.paths.java_path.is_none() {
            Some(self.java_picker.detected_path().to_owned())
        } else {
            None
        };
        self.save_pending = true;
        self.error = None;
    }

    fn handle_java_picker_key(&mut self, key: &KeyEvent) {
        self.java_picker.initialize();
        let count = self.java_picker.labels().len();
        match key.code {
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Left => self.java_picker_open = false,
            KeyCode::Char('a') => {
                self.toggle_auto_java();
                self.java_picker_open = false;
            }
            KeyCode::Char('j') | KeyCode::Down if count > 0 => {
                self.java_picker.selected = (self.java_picker.selected + 1).min(count - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.java_picker.selected = self.java_picker.selected.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                match self.java_picker.selected_choice() {
                    JavaChoice::Installation(path) => {
                        if self.config.paths.java_path.as_deref() != Some(&path) {
                            self.config.paths.java_path = Some(path);
                            self.save_pending = true;
                        }
                    }
                }
                self.java_picker_open = false;
            }
            _ => {}
        }
    }

    fn adjust_selected_memory(&mut self, forward: bool) {
        let value = if self.selected == 2 {
            &self.config.defaults.memory_min
        } else {
            &self.config.defaults.memory_max
        };
        let value = adjust_memory(value, forward);
        if self.selected == 2 {
            self.config.defaults.memory_min = value.clone();
            if memory_kib(&value) > memory_kib(&self.config.defaults.memory_max) {
                self.config.defaults.memory_max = value;
            }
        } else {
            self.config.defaults.memory_max = value.clone();
            if memory_kib(&value) < memory_kib(&self.config.defaults.memory_min) {
                self.config.defaults.memory_min = value;
            }
        }
        self.save_pending = true;
        self.error = None;
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> Action {
        let action = self.handle_key_inner(key);
        if !matches!(action, Action::None) {
            return action;
        }
        if let Some(error) = self.error.take() {
            return Action::Error(error);
        }
        if self.save_pending && self.editing.is_none() {
            self.save_pending = false;
            return Action::Save(
                Box::new(self.config.clone()),
                self.theme.theme.clone(),
                self.theme.border_style.clone(),
            );
        }
        Action::None
    }

    fn handle_key_inner(&mut self, key: &KeyEvent) -> Action {
        if self.java_picker_open {
            self.handle_java_picker_key(key);
            return Action::None;
        }
        if self.theme_picker {
            self.handle_theme_picker_key(key);
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
            KeyCode::Char('j') | KeyCode::Down => self.selected = (self.selected + 1).min(4),
            KeyCode::Char('k') | KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Char('h') | KeyCode::Left if self.selected == 1 => self.cycle_border(false),
            KeyCode::Char('l') | KeyCode::Right if self.selected == 1 => self.cycle_border(true),
            KeyCode::Char('h') | KeyCode::Left if matches!(self.selected, 2 | 3) => {
                self.adjust_selected_memory(false);
            }
            KeyCode::Char('l') | KeyCode::Right if matches!(self.selected, 2 | 3) => {
                self.adjust_selected_memory(true);
            }
            KeyCode::Enter => match self.selected {
                0 => self.theme_picker = true,
                1 => self.cycle_border(true),
                2 | 3 => {
                    self.editing = Some(new_text_area(vec![self.value(self.selected)]));
                }
                4 => self.open_java_picker(),
                field => self.editing = Some(new_text_area(vec![self.value(field)])),
            },
            KeyCode::Char('a') if self.selected == 4 => {
                self.toggle_auto_java();
            }
            KeyCode::Char('c') if self.selected == 4 => {
                self.editing = Some(new_text_area(vec![self.value(4)]));
            }
            KeyCode::Char('E') => {
                let file = if self.selected <= 1 {
                    "theme.toml"
                } else {
                    "config.toml"
                };
                return Action::OpenRaw(crate::config::get_config_path().join(file));
            }
            KeyCode::Esc => return Action::Close,
            _ => {}
        }
        Action::None
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

fn available_themes() -> Vec<String> {
    let mut themes: Vec<String> = ratatui_themekit::available_theme_ids()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let theme_dir = crate::config::get_config_path().join("theme");
    if let Ok(entries) = std::fs::read_dir(theme_dir) {
        themes.extend(entries.flatten().filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
                .then(|| path.file_stem()?.to_str().map(str::to_owned))
                .flatten()
        }));
    }
    themes.sort();
    themes.dedup();
    let current = crate::config::theme::current_theme_config().theme;
    if !themes.contains(&current) {
        themes.push(current);
        themes.sort();
    }
    themes
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
    let height = if state.theme_picker || state.java_picker_open {
        (area.height * 2 / 3).max(10)
    } else {
        7
    };
    let width = if state.java_picker_open { 72 } else { 52 };
    area.centered(
        ratatui::layout::Constraint::Percentage(width),
        ratatui::layout::Constraint::Length(height.min(area.height.saturating_sub(4))),
    )
}

pub fn render(frame: &mut Frame, area: Rect, state: &mut State) {
    if state.java_picker_open {
        state.java_picker.initialize();
    }
    let theme = THEME.as_ref();
    frame.render_widget(Clear, area);
    let keybinds = if state.editing.is_some() {
        super::keybind_line(&[("Enter", " apply"), ("Esc", " cancel")])
    } else if state.java_picker_open {
        super::keybind_line(&[("a", " auto"), ("h", " back"), ("Enter", " select")])
    } else if state.theme_picker {
        super::keybind_line(&[("h", " back"), ("Enter", " select")])
    } else if matches!(state.selected, 2 | 3) {
        super::keybind_line(&[("h/l", " adjust"), ("Enter", " exact"), ("Esc", " back")])
    } else if state.selected == 4 {
        super::keybind_line(&[
            ("Enter", " runtimes"),
            ("a", " auto"),
            ("c", " custom"),
            ("Esc", " back"),
        ])
    } else if state.selected == 0 {
        super::keybind_line(&[
            ("j/k", ""),
            ("Enter", " select"),
            ("E", " raw"),
            ("Esc", " back"),
        ])
    } else if state.selected == 1 {
        super::keybind_line(&[
            ("h/l", " adjust"),
            ("Enter", " next"),
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
    let title = if state.theme_picker {
        " Theme "
    } else if state.java_picker_open {
        " Java Runtime "
    } else {
        " Launcher Settings "
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
    if state.theme_picker {
        render_picker(frame, inner, &state.themes, state.theme_index);
        return;
    }
    if state.java_picker_open {
        render_java_picker(frame, inner, state);
        return;
    }
    render_settings_list(frame, inner, state);
}

fn render_picker(frame: &mut Frame, area: Rect, values: &[String], selected: usize) {
    let theme = THEME.as_ref();
    let items = values
        .iter()
        .map(|name| {
            ListItem::new(Line::from(Span::styled(
                name.clone(),
                Style::default().fg(theme.text()),
            )))
        })
        .collect();
    super::select_list::render(items, selected, area, frame.buffer_mut());
}

fn render_java_picker(frame: &mut Frame, area: Rect, state: &mut State) {
    let theme = THEME.as_ref();
    let mut list_area = area;
    if let Some(status) = state.java_picker.take_status() {
        match status {
            Ok(message) => {
                frame.render_widget(
                    Paragraph::new(message).style(Style::default().fg(theme.text_dim())),
                    Rect { height: 1, ..area },
                );
                list_area.y = list_area.y.saturating_add(1);
                list_area.height = list_area.height.saturating_sub(1);
            }
            Err(error) => crate::feedback::errors::push_message(tracing::Level::ERROR, error),
        }
    }
    super::select_list::render(
        state.java_picker.items(),
        state.java_picker.selected,
        list_area,
        frame.buffer_mut(),
    );
}

fn render_settings_list(frame: &mut Frame, area: Rect, state: &State) {
    let labels = ["Theme", "Border style", "Memory min", "Memory max", "Java"];
    let lines = labels
        .iter()
        .enumerate()
        .map(|(index, label)| global_field_line(state, index, label))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), area);

    for field in [2, 3] {
        let value = state.value(field);
        render_memory_gauge(
            frame,
            Rect {
                x: area.x.saturating_add(20),
                y: area.y.saturating_add(field as u16),
                width: area.width.saturating_sub(21),
                height: 1,
            },
            &value,
            value.clone(),
            state.selected == field,
        );
    }

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

fn global_field_line(state: &State, index: usize, label: &str) -> Line<'static> {
    let theme = THEME.as_ref();
    let selected = index == state.selected;
    let editing = selected && state.editing.is_some();
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
            if editing || matches!(index, 2 | 3) {
                String::new()
            } else {
                state.display_value(index)
            },
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
    ];
    if index == 4 && state.config.paths.java_path.is_none() && !editing {
        spans.extend([Span::raw("  "), auto_label()]);
    }
    Line::from(spans).style(Style::default().bg(if selected {
        theme.stripe()
    } else {
        theme.surface()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_memory_uses_slider_and_java_uses_picker() {
        let mut state = State::new();
        state.selected = 2;
        let original = state.config.defaults.memory_min.clone();
        assert!(matches!(
            state.handle_key(&KeyEvent::from(KeyCode::Char('l'))),
            Action::Save(..)
        ));
        assert_ne!(state.config.defaults.memory_min, original);
        assert!(state.editing.is_none());

        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert!(state.editing.is_some());

        state.editing = None;
        state.selected = 4;
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert!(state.java_picker_open);
    }

    #[test]
    fn java_auto_mode_toggles_to_and_from_the_detected_path() {
        let mut state = State::new();
        state.selected = 4;
        state.config.paths.java_path = None;

        assert!(matches!(
            state.handle_key(&KeyEvent::from(KeyCode::Char('a'))),
            Action::Save(..)
        ));
        assert_eq!(
            state.config.paths.java_path.as_deref(),
            Some(state.java_picker.detected_path())
        );

        assert!(matches!(
            state.handle_key(&KeyEvent::from(KeyCode::Char('a'))),
            Action::Save(..)
        ));
        assert_eq!(state.config.paths.java_path, None);
    }
}
