// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// Reusable interactive controls shared by instance and launcher settings.

use std::sync::{Arc, Mutex};

use ratatui::{Frame, layout::Rect, style::Style, text::Span, widgets::Gauge};

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
    let gauge = Gauge::default()
        .ratio(ratio)
        .use_unicode(true)
        .label(Span::styled(
            label,
            Style::default().fg(if selected {
                theme.text()
            } else {
                theme.text_dim()
            }),
        ))
        .gauge_style(
            Style::default()
                .fg(if selected {
                    theme.accent()
                } else {
                    theme.text_dim()
                })
                .bg(theme.background()),
        );
    frame.render_widget(gauge, area);
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
