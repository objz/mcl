// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// modal editor for launcher-wide settings.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use ratatui_textarea::{CursorMove, TextArea};

use crate::{
    config::{
        Config,
        theme::{BORDER_STYLE, BorderStyle, THEME, ThemeConfig},
    },
    instance::models::normalize_memory_value,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresetPicker {
    Border,
    Java,
}

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
    preset_picker: Option<PresetPicker>,
    preset_index: usize,
    detected_java: String,
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
            preset_picker: None,
            preset_index: 0,
            detected_java: crate::instance::java::detect_java_path(),
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
            1 => match &self.theme.border_style {
                BorderStyle::Rounded => "╭─╮ rounded".to_owned(),
                BorderStyle::Plain => "┌─┐ plain".to_owned(),
                BorderStyle::Double => "╔═╗ double".to_owned(),
                BorderStyle::Thick => "┏━┓ thick".to_owned(),
            },
            4 if self
                .config
                .paths
                .java_path
                .as_deref()
                .is_none_or(str::is_empty) =>
            {
                "auto-detect".to_owned()
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

    fn preset_values_for(&self, picker: PresetPicker) -> Vec<String> {
        match picker {
            PresetPicker::Border => vec![
                "╭─╮ rounded".to_owned(),
                "┌─┐ plain".to_owned(),
                "╔═╗ double".to_owned(),
                "┏━┓ thick".to_owned(),
            ],
            PresetPicker::Java => {
                let mut values = vec!["automatic detection".to_owned()];
                if !self.detected_java.is_empty() {
                    values.push(self.detected_java.clone());
                }
                if let Some(current) = &self.config.paths.java_path
                    && !values.contains(current)
                {
                    values.push(current.clone());
                }
                values.push("custom path…".to_owned());
                values
            }
        }
    }

    fn preset_values(&self) -> Vec<String> {
        self.preset_picker
            .map_or_else(Vec::new, |picker| self.preset_values_for(picker))
    }

    fn open_preset_picker(&mut self, picker: PresetPicker) {
        self.preset_picker = Some(picker);
        let current = match picker {
            PresetPicker::Border => None,
            PresetPicker::Java => self.config.paths.java_path.as_deref(),
        };
        self.preset_index = self
            .preset_values_for(picker)
            .iter()
            .position(|value| match (picker, current) {
                (PresetPicker::Border, _) => value == &self.display_value(1),
                (PresetPicker::Java, None) => value == "automatic detection",
                (_, Some(current)) => value == current,
            })
            .unwrap_or(0);
    }

    fn apply_preset(&mut self) {
        let selected = self
            .preset_values()
            .get(self.preset_index)
            .cloned()
            .unwrap_or_default();
        match self.preset_picker {
            Some(PresetPicker::Border) => {
                let previous = self.theme.border_style.clone();
                self.theme.border_style = match self.preset_index {
                    0 => BorderStyle::Rounded,
                    1 => BorderStyle::Plain,
                    2 => BorderStyle::Double,
                    _ => BorderStyle::Thick,
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
            Some(PresetPicker::Java) if selected == "custom path…" => {
                self.editing = Some(new_text_area(vec![self.value(4)]));
            }
            Some(PresetPicker::Java) => {
                self.config.paths.java_path =
                    (selected != "automatic detection").then_some(selected);
                self.config_dirty = true;
            }
            None => {}
        }
        self.preset_picker = None;
    }

    fn handle_preset_key(&mut self, key: &KeyEvent) {
        let count = self.preset_values().len();
        match key.code {
            KeyCode::Esc => self.preset_picker = None,
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Right if count > 0 => {
                self.preset_index = (self.preset_index + 1).min(count - 1);
            }
            KeyCode::Char('k') | KeyCode::Up | KeyCode::Left => {
                self.preset_index = self.preset_index.saturating_sub(1);
            }
            KeyCode::Enter => self.apply_preset(),
            _ => {}
        }
    }

    fn rotate_preset(&mut self, picker: PresetPicker, forward: bool) {
        self.open_preset_picker(picker);
        let count = self.preset_values().len().saturating_sub(1);
        if count > 0 {
            self.preset_index = if forward {
                (self.preset_index + 1) % count
            } else {
                (self.preset_index + count - 1) % count
            };
            self.apply_preset();
        }
    }

    fn adjust_memory(&mut self, field: usize, forward: bool) {
        let values = ["512M", "1G", "2G", "4G", "6G", "8G", "12G", "16G"];
        let current = if field == 2 {
            &self.config.defaults.memory_min
        } else {
            &self.config.defaults.memory_max
        };
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
        let value = values[next].to_owned();
        if field == 2 {
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
        self.config_dirty = true;
        self.error = None;
    }

    fn cycle_theme(&mut self, forward: bool) {
        let count = self.themes.len();
        if count == 0 {
            return;
        }
        self.theme_index = if forward {
            (self.theme_index + 1) % count
        } else {
            (self.theme_index + count - 1) % count
        };
        self.select_theme();
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
        if self.preset_picker.is_some() {
            self.handle_preset_key(key);
            return Action::None;
        }
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
            KeyCode::Left => match self.selected {
                0 => self.cycle_theme(false),
                1 => self.cycle_border(false),
                2 | 3 => self.adjust_memory(self.selected, false),
                4 => self.rotate_preset(PresetPicker::Java, false),
                _ => {}
            },
            KeyCode::Right => match self.selected {
                0 => self.cycle_theme(true),
                1 => self.cycle_border(true),
                2 | 3 => self.adjust_memory(self.selected, true),
                4 => self.rotate_preset(PresetPicker::Java, true),
                _ => {}
            },
            KeyCode::Enter => match self.selected {
                0 => self.theme_picker = true,
                1 => self.open_preset_picker(PresetPicker::Border),
                2 | 3 => self.adjust_memory(self.selected, true),
                4 => self.open_preset_picker(PresetPicker::Java),
                _ => {}
            },
            KeyCode::Char('c') if matches!(self.selected, 2 | 3) => {
                self.editing = Some(new_text_area(vec![self.value(self.selected)]));
            }
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

pub fn popup_rect(area: Rect, state: &State) -> Rect {
    let height = if state.theme_picker || state.preset_picker.is_some() || state.editing.is_some() {
        14
    } else {
        12
    };
    area.centered(
        ratatui::layout::Constraint::Percentage(68),
        ratatui::layout::Constraint::Length(height.min(area.height.saturating_sub(4))),
    )
}

pub fn render(frame: &mut Frame, area: Rect, state: &State) {
    let theme = THEME.as_ref();
    frame.render_widget(Clear, area);
    let keybinds = if state.theme_picker || state.preset_picker.is_some() {
        super::keybind_line(&[("j/k", " move"), ("Enter", " apply"), ("Esc", " back")])
    } else if matches!(state.selected, 2 | 3) {
        super::keybind_line(&[
            ("j/k", ""),
            ("←/→", " memory"),
            ("c", " custom"),
            ("s", " save"),
            ("E", " raw"),
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
    let mut title = vec![Span::styled(
        " Launcher Settings ",
        Style::default()
            .fg(theme.text())
            .add_modifier(Modifier::BOLD),
    )];
    if state.config_dirty {
        title.push(Span::styled(
            "● modified ",
            Style::default().fg(theme.warning()),
        ));
    }
    let block = Block::default()
        .title(Line::from(title))
        .borders(Borders::ALL)
        .border_type(BORDER_STYLE.to_border_type())
        .border_style(Style::default().fg(theme.text_dim()))
        .style(Style::default().bg(theme.surface()))
        .title_bottom(keybinds);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if state.theme_picker {
        render_picker(frame, inner, " Themes ", &state.themes, state.theme_index);
        return;
    }
    if let Some(picker) = state.preset_picker {
        let title = match picker {
            PresetPicker::Border => " Border Style ",
            PresetPicker::Java => " Java Runtime ",
        };
        render_picker(
            frame,
            inner,
            title,
            &state.preset_values(),
            state.preset_index,
        );
        return;
    }

    let sections = ratatui::layout::Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            ratatui::layout::Constraint::Length(4),
            ratatui::layout::Constraint::Length(5),
            ratatui::layout::Constraint::Min(1),
        ])
        .split(inner);
    render_global_card(
        frame,
        sections[0],
        " Appearance ",
        state,
        &[(0, "Theme"), (1, "Borders")],
    );
    render_global_card(
        frame,
        sections[1],
        " Launch Defaults ",
        state,
        &[(2, "Min memory"), (3, "Max memory"), (4, "Java")],
    );

    if let Some(editor) = state.editing.as_ref() {
        let editor_block = Block::default()
            .title(match state.selected {
                2 => " Custom minimum memory (K/M/G) · Enter apply ",
                3 => " Custom maximum memory (K/M/G) · Enter apply ",
                4 => " Custom Java executable path · Enter apply ",
                _ => " Custom value · Enter apply ",
            })
            .borders(Borders::ALL)
            .border_type(BORDER_STYLE.to_border_type())
            .border_style(Style::default().fg(theme.accent()))
            .style(Style::default().bg(theme.surface()));
        let editor_inner = editor_block.inner(sections[2]);
        frame.render_widget(editor_block, sections[2]);
        frame.render_widget(editor, editor_inner);
    } else {
        let status = if let Some(error) = &state.error {
            Line::from(Span::styled(
                format!("  × {error}"),
                Style::default().fg(theme.error()),
            ))
        } else if state.confirm_close {
            Line::from(Span::styled(
                "  ! Discard unsaved launcher defaults?  [y] yes  [n] no",
                Style::default().fg(theme.warning()),
            ))
        } else {
            Line::from(vec![
                Span::styled("  ◇ Live preview  ", Style::default().fg(theme.info())),
                Span::styled(
                    "theme and border changes apply immediately",
                    Style::default().fg(theme.text_dim()),
                ),
            ])
        };
        frame.render_widget(
            Paragraph::new(status).wrap(Wrap { trim: true }),
            sections[2],
        );
    }
}

fn render_picker(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    values: &[String],
    selected: usize,
) {
    let theme = THEME.as_ref();
    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(theme.accent())))
        .borders(Borders::ALL)
        .border_type(BORDER_STYLE.to_border_type())
        .border_style(Style::default().fg(theme.accent()))
        .style(Style::default().bg(theme.surface()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let visible_rows = inner.height as usize;
    let start = selected.saturating_sub(visible_rows.saturating_sub(1));
    let lines = values
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_rows)
        .map(|(index, name)| {
            let focused = index == selected;
            Line::from(vec![
                Span::styled(
                    if focused { "▶ " } else { "  " },
                    Style::default().fg(theme.accent()),
                ),
                Span::styled(
                    name.clone(),
                    Style::default()
                        .fg(if focused {
                            theme.accent()
                        } else {
                            theme.text()
                        })
                        .add_modifier(if focused {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ])
            .style(Style::default().bg(if focused {
                theme.stripe()
            } else {
                theme.surface()
            }))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_global_card(
    frame: &mut Frame,
    area: Rect,
    title: &'static str,
    state: &State,
    fields: &[(usize, &str)],
) {
    let theme = THEME.as_ref();
    let active = fields.iter().any(|(index, _)| *index == state.selected);
    let block = Block::default()
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
        .style(Style::default().bg(theme.surface()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = fields
        .iter()
        .map(|(index, label)| global_field_line(state, *index, label))
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn global_field_line<'a>(state: &'a State, index: usize, label: &str) -> Line<'a> {
    let theme = THEME.as_ref();
    let selected = index == state.selected;
    let displayed = state.display_value(index);
    let mut spans = vec![
        Span::styled(
            if selected { "▶ " } else { "  " },
            Style::default().fg(theme.accent()),
        ),
        Span::styled(
            format!("{label:<13}"),
            Style::default().fg(theme.text_dim()),
        ),
    ];
    if matches!(index, 2 | 3) {
        spans.push(global_memory_slider(&displayed, selected));
    } else {
        spans.push(Span::styled(
            format!(" ‹ {displayed} › "),
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
        ));
    }
    Line::from(spans).style(Style::default().bg(if selected {
        theme.stripe()
    } else {
        theme.surface()
    }))
}

fn global_memory_slider(value: &str, selected: bool) -> Span<'static> {
    let theme = THEME.as_ref();
    let thresholds = ["512M", "1G", "2G", "4G", "6G", "8G", "12G", "16G"];
    let value_kib = memory_kib(value).unwrap_or_default();
    let step = thresholds
        .iter()
        .position(|threshold| memory_kib(threshold).is_some_and(|limit| value_kib <= limit))
        .unwrap_or(thresholds.len() - 1);
    let filled = step + 1;
    Span::styled(
        format!(
            " ◀ {}{} {value} ▶ ",
            "▰".repeat(filled),
            "▱".repeat(thresholds.len() - filled)
        ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_defaults_use_memory_sliders_and_java_picker() {
        let mut state = State::new();
        state.selected = 2;
        let original = state.config.defaults.memory_min.clone();
        state.handle_key(&KeyEvent::from(KeyCode::Right));
        assert_ne!(state.config.defaults.memory_min, original);
        assert!(state.preset_picker.is_none());
        assert!(state.editing.is_none());

        state.selected = 4;
        state.handle_key(&KeyEvent::from(KeyCode::Enter));
        assert_eq!(state.preset_picker, Some(PresetPicker::Java));
        assert!(state.preset_values().contains(&"custom path…".to_owned()));
    }
}
