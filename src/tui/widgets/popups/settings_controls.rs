// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// Reusable interactive controls shared by instance and launcher settings.

use std::sync::{Arc, Mutex};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{LineGauge, ListItem, Paragraph},
};

use crate::{
    config::theme::THEME, instance::java::JavaInstallation, tui::widgets::popups::LoadState,
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
    Automatic,
    Installation(String),
    Custom,
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
        let mut choices = vec![JavaChoice::Automatic];
        if let LoadState::Loaded(installations) = &*self
            .load
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            choices.extend(installations.iter().map(|installation| {
                JavaChoice::Installation(installation.path.to_string_lossy().into_owned())
            }));
        }
        if let Some(current) = &self.current
            && !choices
                .iter()
                .any(|choice| matches!(choice, JavaChoice::Installation(path) if path == current))
        {
            choices.push(JavaChoice::Installation(current.clone()));
        }
        choices.push(JavaChoice::Custom);
        choices
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
                JavaChoice::Automatic => format!("Automatic  {}", self.detected),
                JavaChoice::Installation(path) => installations
                    .as_ref()
                    .and_then(|items| {
                        items
                            .iter()
                            .find(|item| item.path.to_string_lossy() == path)
                    })
                    .map_or_else(|| format!("Java  {path}"), JavaInstallation::label),
                JavaChoice::Custom => "Custom path…".to_owned(),
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
            .map(|choice| match choice {
                JavaChoice::Automatic => ListItem::new(Line::from(vec![
                    Span::styled("Automatic", Style::default().fg(theme.text())),
                    Span::raw("  "),
                    badge(" Auto ", theme.info()),
                    Span::styled(
                        format!("  {}", self.detected),
                        Style::default().fg(theme.text_dim()),
                    ),
                ])),
                JavaChoice::Installation(path) => {
                    let installation = installations
                        .iter()
                        .find(|installation| installation.path.to_string_lossy() == path);
                    let version = installation
                        .and_then(|installation| installation.version.as_deref())
                        .map_or_else(|| "Java".to_owned(), |version| format!("Java {version}"));
                    let current = self.current.as_deref() == Some(path.as_str());
                    ListItem::new(Line::from(vec![
                        Span::styled(version, Style::default().fg(theme.text())),
                        Span::raw("  "),
                        badge(
                            if current { " Current " } else { " Detected " },
                            if current {
                                theme.success()
                            } else {
                                theme.info()
                            },
                        ),
                        Span::styled(format!("  {path}"), Style::default().fg(theme.text_dim())),
                    ]))
                }
                JavaChoice::Custom => ListItem::new(Line::from(vec![
                    Span::styled("Custom path…", Style::default().fg(theme.text())),
                    Span::raw("  "),
                    badge(" Manual ", theme.warning()),
                ])),
            })
            .collect()
    }

    pub(crate) fn status(&self) -> Option<Result<&'static str, String>> {
        match &*self
            .load
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
        {
            LoadState::Idle => None,
            LoadState::Loading => Some(Ok("Detecting installed Java runtimes…")),
            LoadState::Loaded(installations) if installations.is_empty() => {
                Some(Ok("No additional Java runtimes found."))
            }
            LoadState::Loaded(_) => None,
            LoadState::Error(error) => Some(Err(error.clone())),
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
                self.current
                    .as_ref()
                    .map(|current| JavaChoice::Installation(current.clone()))
            })
            .unwrap_or(JavaChoice::Automatic);
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
            .unwrap_or(JavaChoice::Automatic)
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
            .fg(theme.background())
            .bg(theme.accent())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text()).bg(theme.background())
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(format!(" {label} "), value_style))),
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
                "◆",
                Style::default().fg(if selected {
                    theme.accent()
                } else {
                    theme.text_dim()
                }),
            )),
            Rect {
                x: line_area.x.saturating_add(thumb_offset),
                width: 1,
                ..line_area
            },
        );
    }
}

pub(crate) fn display_resolutions() -> Vec<DisplayResolution> {
    let Ok(displays) = display_info::DisplayInfo::all() else {
        return Vec::new();
    };
    let mut resolutions = Vec::<DisplayResolution>::new();
    for display in displays {
        if display.width == 0 || display.height == 0 {
            continue;
        }
        if let Some(existing) = resolutions.iter_mut().find(|resolution| {
            resolution.width == display.width && resolution.height == display.height
        }) {
            existing.primary |= display.is_primary;
            continue;
        }
        let name = if display.friendly_name.is_empty() {
            display.name
        } else {
            display.friendly_name
        };
        resolutions.push(DisplayResolution {
            width: display.width,
            height: display.height,
            name,
            primary: display.is_primary,
        });
    }
    resolutions.sort_by_key(|resolution| !resolution.primary);
    resolutions
}

pub(crate) fn badge(text: &'static str, color: Color) -> Span<'static> {
    Span::styled(
        text,
        Style::default()
            .fg(THEME.as_ref().background())
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
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
        picker.current = None;
        picker.initialize();
        picker.selected = picker.choices().len() - 1;
        *picker.load.lock().unwrap() = LoadState::Loaded(vec![JavaInstallation {
            path: "/opt/jdk/bin/java".into(),
            version: Some("21".to_owned()),
        }]);

        picker.initialize();

        assert_eq!(picker.selected_choice(), JavaChoice::Custom);
    }
}
