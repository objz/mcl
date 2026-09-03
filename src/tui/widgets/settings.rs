// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// compact, read-only summary of the selected instance. editing lives in the
// instance and launcher settings popups.

use crate::config::{
    SETTINGS,
    theme::{BORDER_STYLE, THEME},
};
use crate::instance::models::InstanceConfig;
use crate::tui::app::FocusedArea;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use super::styled_title;

const LOCAL_PROFILE_LABEL: &str = "instance default";

pub fn render(
    frame: &mut Frame,
    area: Rect,
    focused: FocusedArea,
    instance: Option<&InstanceConfig>,
) {
    let theme = THEME.as_ref();
    let color = if focused == FocusedArea::Settings {
        theme.accent()
    } else {
        theme.border()
    };

    let mut block = Block::default()
        .title(styled_title("Settings", true))
        .borders(Borders::ALL)
        .border_type(BORDER_STYLE.to_border_type())
        .border_style(Style::default().fg(color));

    let keybind_line = if focused == FocusedArea::Settings {
        let keybinds: &[(&str, &str)] = &[("E", " instance"), ("G", " launcher"), ("Esc", " back")];
        Some(super::popups::keybind_line_fitted(
            keybinds,
            area.width.saturating_sub(2),
        ))
    } else {
        None
    };
    if let Some(line) = keybind_line {
        block = block.title_bottom(line);
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    render_instance_info(frame, inner, instance);
}

fn render_instance_info(frame: &mut Frame, area: Rect, instance: Option<&InstanceConfig>) {
    let theme = THEME.as_ref();
    let label_style = Style::default().fg(theme.text_dim());
    let value_style = Style::default()
        .fg(theme.text())
        .add_modifier(Modifier::BOLD);

    let Some(inst) = instance else {
        frame.render_widget(
            Paragraph::new("No instance selected.").style(Style::default().fg(theme.text_dim())),
            area,
        );
        return;
    };

    let settings = SETTINGS.read();
    let memory_min = inst
        .memory_min
        .as_deref()
        .unwrap_or(&settings.defaults.memory_min);
    let memory_max = inst
        .memory_max
        .as_deref()
        .unwrap_or(&settings.defaults.memory_max);
    let active_style = value_style;
    let desktop = if crate::instance::desktop::exists(&inst.name) {
        "yes"
    } else {
        "no"
    };
    let java_source = if inst
        .java_path
        .as_deref()
        .is_some_and(|path| !path.is_empty())
    {
        "instance java"
    } else if settings.paths.effective_java_path().is_some() {
        "global java"
    } else {
        "auto java"
    };
    let lines = vec![
        Line::from(vec![
            Span::styled("Version  ", label_style),
            Span::styled(
                format!("{} / {}", inst.game_version, inst.loader),
                active_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("Runtime  ", label_style),
            Span::styled(
                format!("{memory_min}-{memory_max}, {java_source}"),
                active_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("Profile  ", label_style),
            Span::styled(
                inst.config_sync_profile
                    .as_deref()
                    .unwrap_or(LOCAL_PROFILE_LABEL),
                active_style,
            ),
            Span::styled(" / desk ", label_style),
            Span::styled(desktop, active_style),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines), area);
}
