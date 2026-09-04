// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// Reusable interactive controls shared by instance and launcher settings.

use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{LineGauge, ListItem, Paragraph},
};
use ratatui_textarea::TextArea;

use crate::{
    config::theme::THEME,
    instance::java::JavaInstallation,
    tui::widgets::{popups::LoadState, status_badge},
};

const MEMORY_STEPS: [&str; 12] = [
    "512M", "1G", "2G", "3G", "4G", "6G", "8G", "12G", "16G", "24G", "32G", "64G",
];

#[derive(Debug, Clone)]
pub(crate) struct JavaPicker {
    load: Arc<Mutex<LoadState<Vec<JavaInstallation>>>>,
    current: Option<String>,
    detected: String,
    pub selected: usize,
    previous_choices: Vec<JavaChoice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JavaChoice {
    Installation(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DisplayResolution {
    pub width: u32,
    pub height: u32,
    pub name: String,
    pub primary: bool,
}

impl JavaPicker {
    pub(crate) fn new() -> Self {
        Self {
            load: Arc::new(Mutex::new(LoadState::Idle)),
            current: None,
            detected: crate::instance::java::detect_java_path(),
            selected: 0,
            previous_choices: Vec::new(),
        }
    }

    pub(crate) fn open(&mut self, current: Option<&str>) {
        self.current = current.map(str::to_owned);
        self.previous_choices.clear();
        let mut load = self
            .load
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !matches!(*load, LoadState::Idle | LoadState::Error(_)) {
            return;
        }
        *load = LoadState::Loading;
        drop(load);
        let target = self.load.clone();
        let discover = move || {
            let installations = crate::instance::java::discover_installations();
            *target
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) =
                LoadState::Loaded(installations);
            crate::feedback::request_redraw();
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn_blocking(discover);
        } else {
            *self
                .load
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = LoadState::Loaded(Vec::new());
        }
    }

    pub(crate) fn choices(&self) -> Vec<JavaChoice> {
        let mut paths = Vec::new();
        if let LoadState::Loaded(installations) = &*self
            .load
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            paths.extend(
                installations
                    .iter()
                    .map(|installation| installation.path.to_string_lossy().into_owned()),
            );
        }
        if !paths.contains(&self.detected) {
            paths.insert(0, self.detected.clone());
        }
        if let Some(current) = &self.current
            && !paths.contains(current)
        {
            paths.push(current.clone());
        }
        paths.into_iter().map(JavaChoice::Installation).collect()
    }

    pub(crate) fn labels(&self) -> Vec<String> {
        let installations = match &*self
            .load
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            LoadState::Loaded(installations) => Some(installations.clone()),
            _ => None,
        };
        self.choices()
            .into_iter()
            .map(|choice| match choice {
                JavaChoice::Installation(path) => installations
                    .as_ref()
                    .and_then(|items| {
                        items
                            .iter()
                            .find(|item| item.path.to_string_lossy() == path)
                    })
                    .map_or_else(|| format!("Java  {path}"), JavaInstallation::label),
            })
            .collect()
    }

    pub(crate) fn items(&self) -> Vec<ListItem<'static>> {
        let theme = THEME.as_ref();
        let installations = match &*self
            .load
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            LoadState::Loaded(installations) => installations.clone(),
            _ => Vec::new(),
        };
        self.choices()
            .into_iter()
            .enumerate()
            .map(|(index, choice)| match choice {
                JavaChoice::Installation(path) => {
                    let installation = installations
                        .iter()
                        .find(|installation| installation.path.to_string_lossy() == path);
                    let version = installation
                        .and_then(|installation| installation.version.as_deref())
                        .map_or_else(|| "Java".to_owned(), |version| format!("Java {version}"));
                    let selected = index == self.selected;
                    let mut spans = vec![
                        Span::styled(
                            version,
                            Style::default().fg(if selected {
                                theme.accent()
                            } else {
                                theme.text()
                            }),
                        ),
                        Span::styled(
                            format!("  {path}"),
                            Style::default().fg(if selected {
                                theme.accent()
                            } else {
                                theme.text_dim()
                            }),
                        ),
                    ];
                    if self.current.is_none() && path == self.detected {
                        spans.extend([Span::raw("  "), auto_label()]);
                    }
                    ListItem::new(Line::from(spans))
                }
            })
            .collect()
    }

    pub(crate) fn take_status(&mut self) -> Option<Result<&'static str, String>> {
        let mut load = self
            .load
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &*load {
            LoadState::Idle => None,
            LoadState::Loading => Some(Ok("Detecting installed Java runtimes…")),
            LoadState::Loaded(installations) if installations.is_empty() => {
                Some(Ok("No additional Java runtimes found."))
            }
            LoadState::Loaded(_) => None,
            LoadState::Error(error) => {
                let error = error.clone();
                *load = LoadState::Loaded(Vec::new());
                Some(Err(error))
            }
        }
    }

    pub(crate) fn initialize(&mut self) {
        let choices = self.choices();
        if choices == self.previous_choices {
            return;
        }
        let selected = self
            .previous_choices
            .get(self.selected)
            .cloned()
            .or_else(|| {
                Some(JavaChoice::Installation(
                    self.current
                        .clone()
                        .unwrap_or_else(|| self.detected.clone()),
                ))
            })
            .unwrap_or_else(|| JavaChoice::Installation(self.detected.clone()));
        self.selected = choices
            .iter()
            .position(|choice| choice == &selected)
            .unwrap_or(0);
        self.previous_choices = choices;
    }

    pub(crate) fn selected_choice(&self) -> JavaChoice {
        self.choices()
            .get(self.selected)
            .cloned()
            .unwrap_or_else(|| JavaChoice::Installation(self.detected.clone()))
    }

    pub(crate) fn detected_path(&self) -> &str {
        &self.detected
    }
}

impl Default for JavaPicker {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn memory_kib(value: &str) -> Option<u64> {
    let normalized = crate::instance::models::normalize_memory_value(value)?;
    let (number, suffix) = normalized.split_at(normalized.len().saturating_sub(1));
    let number = number.parse::<u64>().ok()?;
    match suffix {
        "K" => Some(number),
        "M" => number.checked_mul(1024),
        "G" => number.checked_mul(1024 * 1024),
        _ => None,
    }
}

pub(crate) fn adjust_memory(value: &str, forward: bool) -> String {
    let current = memory_kib(value).unwrap_or_default();
    let exact = MEMORY_STEPS.iter().position(|step| *step == value);
    let index = match (exact, forward) {
        (Some(index), true) => (index + 1).min(MEMORY_STEPS.len() - 1),
        (Some(index), false) => index.saturating_sub(1),
        (None, true) => MEMORY_STEPS
            .iter()
            .position(|step| memory_kib(step).is_some_and(|amount| amount > current))
            .unwrap_or(MEMORY_STEPS.len() - 1),
        (None, false) => MEMORY_STEPS
            .iter()
            .rposition(|step| memory_kib(step).is_some_and(|amount| amount < current))
            .unwrap_or(0),
    };
    MEMORY_STEPS[index].to_owned()
}

pub(crate) fn render_memory_gauge(
    frame: &mut Frame,
    area: Rect,
    value: &str,
    label: String,
    selected: bool,
) {
    let theme = THEME.as_ref();
    let current = memory_kib(value).unwrap_or_default();
    let index = MEMORY_STEPS
        .iter()
        .position(|step| memory_kib(step).is_some_and(|amount| current <= amount))
        .unwrap_or(MEMORY_STEPS.len() - 1);
    let ratio = (index + 1) as f64 / MEMORY_STEPS.len() as f64;
    let value_width = 17.min(area.width);
    let value_area = Rect {
        width: value_width,
        ..area
    };
    let line_area = Rect {
        x: area.x.saturating_add(value_width).saturating_add(1),
        width: area.width.saturating_sub(value_width.saturating_add(1)),
        ..area
    };
    let value_style = if selected {
        Style::default()
            .fg(theme.accent())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text())
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(label, value_style))),
        value_area,
    );

    let gauge = LineGauge::default()
        .ratio(ratio)
        .label("")
        .filled_symbol("━")
        .unfilled_symbol("─")
        .filled_style(Style::default().fg(if selected {
            theme.accent()
        } else {
            theme.text_dim()
        }))
        .unfilled_style(Style::default().fg(theme.border()));
    frame.render_widget(gauge, line_area);
    if line_area.width > 0 {
        let thumb_offset = ((line_area.width.saturating_sub(1)) as f64 * ratio).round() as u16;
        frame.render_widget(
            Paragraph::new(Span::styled(
                if thumb_offset + 1 < line_area.width {
                    "◆ "
                } else {
                    "◆"
                },
                Style::default().fg(if selected {
                    theme.accent()
                } else {
                    theme.text_dim()
                }),
            )),
            Rect {
                x: line_area.x.saturating_add(thumb_offset),
                width: 2.min(line_area.width.saturating_sub(thumb_offset)),
                ..line_area
            },
        );
    }
}

pub(crate) fn handle_text_area_input(input: &mut TextArea<'_>, key: &KeyEvent) {
    if key.code == KeyCode::Backspace && key.modifiers.contains(KeyModifiers::CONTROL) {
        input.delete_word();
    } else {
        input.input(*key);
    }
}

pub(crate) fn auto_label() -> Span<'static> {
    status_badge("Auto", THEME.as_ref().success())
}

pub(crate) fn display_resolutions() -> Vec<DisplayResolution> {
    let Ok(displays) = display_info::DisplayInfo::all() else {
        return Vec::new();
    };
    let mut resolutions = displays
        .into_iter()
        .filter(|display| display.width > 0 && display.height > 0)
        .map(|display| {
            let name = if display.name.is_empty() {
                display.friendly_name
            } else {
                display.name
            };
            DisplayResolution {
                width: display.width,
                height: display.height,
                name,
                primary: display.is_primary,
            }
        })
        .collect::<Vec<_>>();
    resolutions.sort_by_key(|resolution| !resolution.primary);
    resolutions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_steps_move_and_clamp() {
        assert_eq!(adjust_memory("2G", true), "3G");
        assert_eq!(adjust_memory("2G", false), "1G");
        assert_eq!(adjust_memory("64G", true), "64G");
        assert_eq!(adjust_memory("512M", false), "512M");
    }

    #[test]
    fn java_picker_preserves_semantic_selection_when_results_arrive() {
        let mut picker = JavaPicker::new();
        picker.current = Some("/opt/jdk/bin/java".to_owned());
        picker.initialize();
        *picker.load.lock().unwrap() = LoadState::Loaded(vec![JavaInstallation {
            path: "/opt/jdk/bin/java".into(),
            version: Some("21".to_owned()),
        }]);

        picker.initialize();

        assert_eq!(
            picker.selected_choice(),
            JavaChoice::Installation("/opt/jdk/bin/java".to_owned())
        );
    }

    #[test]
    fn settings_text_input_deletes_the_previous_word() {
        let mut input = TextArea::from(["one two"]);
        input.move_cursor(ratatui_textarea::CursorMove::End);

        handle_text_area_input(
            &mut input,
            &KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
        );

        assert_eq!(input.lines(), ["one "]);
    }

    #[test]
    fn memory_thumb_clears_the_first_unfilled_cell() {
        let backend = ratatui::backend::TestBackend::new(40, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_memory_gauge(frame, frame.area(), "8G", "8G".to_owned(), true))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let thumb = (0..40)
            .find(|x| buffer[(*x, 0)].symbol() == "◆")
            .expect("slider thumb");

        assert_eq!(buffer[(thumb + 1, 0)].symbol(), " ");
    }

    #[test]
    fn memory_value_uses_the_existing_row_background() {
        let backend = ratatui::backend::TestBackend::new(40, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        let surface = THEME.as_ref().surface();
        terminal
            .draw(|frame| {
                frame.render_widget(
                    ratatui::widgets::Block::default().style(Style::default().bg(surface)),
                    frame.area(),
                );
                render_memory_gauge(frame, frame.area(), "6G", "6G".to_owned(), false);
            })
            .unwrap();

        assert_eq!(terminal.backend().buffer()[(1, 0)].bg, surface);
    }
}
