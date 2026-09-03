// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// modal editor for launcher-wide settings.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use ratatui_textarea::{CursorMove, TextArea};

use crate::{
    config::{
        Config,
        theme::{BORDER_STYLE, BorderStyle, THEME, ThemeConfig},
    },
    instance::models::normalize_memory_value,
};

pub struct State {
    pub config: Config,
    pub theme: ThemeConfig,
    selected: usize,
    editing: Option<TextArea<'static>>,
    error: Option<String>,
    config_dirty: bool,
    confirm_close: bool,
    themes: Vec<String>,
    theme_picker: bool,
    theme_index: usize,
}

pub enum Action {
    None,
    Save(Box<Config>, String, BorderStyle),
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
            config_dirty: false,
            confirm_close: false,
            themes,
            theme_picker: false,
            theme_index,
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
        if field == 4 && self.config.paths.java_path.is_none() {
            "auto-detect".to_owned()
        } else {
            self.value(field)
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
                "memory must be a positive number with K, M, or G".to_owned(),
            ),
            2 => {
                self.config.defaults.memory_min = normalize_memory_value(value).unwrap();
                self.config_dirty = true;
            }
            3 => {
                self.config.defaults.memory_max = normalize_memory_value(value).unwrap();
                self.config_dirty = true;
            }
            4 => {
                self.config.paths.java_path = (!value.is_empty()).then(|| value.to_owned());
                self.config_dirty = true;
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
        }
    }

    fn handle_theme_picker_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Esc => self.theme_picker = false,
            KeyCode::Char('j') | KeyCode::Down => {
                self.theme_index = (self.theme_index + 1).min(self.themes.len() - 1);
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.theme_index = self.theme_index.saturating_sub(1);
            }
            KeyCode::Enter => {
                self.select_theme();
                if self.error.is_none() {
                    self.theme_picker = false;
                }
            }
            _ => {}
        }
    }

    fn cycle_border(&mut self) {
        let previous = self.theme.border_style.clone();
        self.theme.border_style = match self.theme.border_style {
            BorderStyle::Rounded => BorderStyle::Plain,
            BorderStyle::Plain => BorderStyle::Double,
            BorderStyle::Double => BorderStyle::Thick,
            BorderStyle::Thick => BorderStyle::Rounded,
        };
        self.error = None;
        if let Err(error) = crate::config::theme::apply_theme(
            self.theme.theme.clone(),
            self.theme.border_style.clone(),
        ) {
            self.error = Some(error.to_string());
            self.theme.border_style = previous;
        }
    }

    fn validate_before_save(&mut self) -> bool {
        self.error = None;
        let min = memory_kib(&self.config.defaults.memory_min);
        let max = memory_kib(&self.config.defaults.memory_max);
        if min.zip(max).is_some_and(|(min, max)| min > max) {
            self.error = Some("minimum memory cannot exceed maximum memory".to_owned());
        }
        self.error.is_none()
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> Action {
        if self.theme_picker {
            self.handle_theme_picker_key(key);
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
            KeyCode::Char('j') | KeyCode::Down => self.selected = (self.selected + 1).min(4),
            KeyCode::Char('k') | KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Enter => match self.selected {
                0 => self.theme_picker = true,
                1 => self.cycle_border(),
                field => self.editing = Some(new_text_area(vec![self.value(field)])),
            },
            KeyCode::Char('s') => {
                if self.validate_before_save() {
                    return Action::Save(
                        Box::new(self.config.clone()),
                        self.theme.theme.clone(),
                        self.theme.border_style.clone(),
                    );
                }
            }
            KeyCode::Char('E') if self.config_dirty => {
                self.error = Some(
                    "save or discard launcher defaults before opening the raw file".to_owned(),
                );
            }
            KeyCode::Char('E') => {
                let file = if self.selected <= 1 {
                    "theme.toml"
                } else {
                    "config.toml"
                };
                return Action::OpenRaw(crate::config::get_config_path().join(file));
            }
            KeyCode::Esc if self.config_dirty => self.confirm_close = true,
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

pub fn render(frame: &mut Frame, area: Rect, state: &State) {
    let theme = THEME.as_ref();
    frame.render_widget(Clear, area);
    let keybinds = if state.theme_picker {
        super::keybind_line(&[("j/k", " move"), ("Enter", " apply"), ("Esc", " back")])
    } else {
        super::keybind_line(&[
            ("j/k", " field"),
            ("Enter", " edit"),
            ("s", " save"),
            ("E", " raw"),
            ("Esc", " back"),
        ])
    };
    let block = Block::default()
        .title(if state.config_dirty {
            " Launcher settings * "
        } else {
            " Launcher settings "
        })
        .borders(Borders::ALL)
        .border_type(BORDER_STYLE.to_border_type())
        .border_style(Style::default().fg(theme.accent()))
        .style(Style::default().bg(theme.background()))
        .title_bottom(keybinds);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if state.theme_picker {
        let mut lines = vec![Line::from(Span::styled(
            "Select theme",
            Style::default()
                .fg(theme.text())
                .add_modifier(Modifier::BOLD),
        ))];
        let visible_rows = inner.height.saturating_sub(1) as usize;
        let start = state
            .theme_index
            .saturating_sub(visible_rows.saturating_sub(1));
        for (index, name) in state
            .themes
            .iter()
            .enumerate()
            .skip(start)
            .take(visible_rows)
        {
            let selected = index == state.theme_index;
            lines.push(Line::from(vec![
                Span::styled(
                    if selected { "▸ " } else { "  " },
                    Style::default().fg(theme.accent()),
                ),
                Span::styled(
                    name.clone(),
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
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }
    let labels = [
        "Theme",
        "Border style",
        "Default memory min",
        "Default memory max",
        "Java path",
    ];
    let mut lines = Vec::new();
    for (index, label) in labels.iter().enumerate() {
        let selected = index == state.selected;
        let displayed = if selected {
            state.editing.as_ref().map_or_else(
                || state.display_value(index),
                |_| "editing below".to_owned(),
            )
        } else {
            state.display_value(index)
        };
        lines.push(Line::from(vec![
            Span::styled(
                if selected { "▸ " } else { "  " },
                Style::default().fg(theme.accent()),
            ),
            Span::styled(
                format!("{label:<20}"),
                Style::default().fg(theme.text_dim()),
            ),
            Span::styled(
                displayed,
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
    if let Some(error) = &state.error {
        lines.push(Line::from(Span::styled(
            error,
            Style::default().fg(theme.error()),
        )));
    } else if state.confirm_close {
        lines.push(Line::from(Span::styled(
            "Discard unsaved launcher defaults? [y] yes  [n] no",
            Style::default().fg(theme.warning()),
        )));
    }
    frame.render_widget(Paragraph::new(lines), inner);
    if let Some(editor) = state.editing.as_ref() {
        let editor_area = Rect {
            x: inner.x,
            y: inner.y.saturating_add(7),
            width: inner.width,
            height: 3.min(inner.height.saturating_sub(7)).max(1),
        };
        let editor_block = Block::default()
            .title(" Value — Enter applies ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent()));
        let editor_inner = editor_block.inner(editor_area);
        frame.render_widget(editor_block, editor_area);
        frame.render_widget(editor, editor_inner);
    }
}
