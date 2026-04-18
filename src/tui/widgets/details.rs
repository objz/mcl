use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::instance::models::InstanceConfig;
use crate::tui::app::FocusedArea;
use crate::config::theme::{THEME, BORDER_STYLE};

use super::styled_title;

pub fn render(
    frame: &mut Frame,
    area: Rect,
    focused: FocusedArea,
    instance: Option<&InstanceConfig>,
    _instances_dir: &std::path::Path,
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

    if focused == FocusedArea::Settings {
        let lines = super::popups::keybind_lines_wrapped(
            &[
                ("e", " edit instance"),
                ("g", " edit global"),
                ("d", " desktop"),
            ],
            area.width.saturating_sub(2),
        );
        for line in lines {
            block = block.title_bottom(line);
        }
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let Some(inst) = instance else {
        frame.render_widget(
            Paragraph::new("No instance selected.")
                .style(Style::default().fg(theme.text_dim())),
            inner,
        );
        return;
    };

    let label_style = Style::default().fg(theme.text_dim());
    let value_style = Style::default()
        .fg(theme.text())
        .add_modifier(Modifier::BOLD);
    let dim_style = Style::default().fg(theme.text_dim());

    let memory_min = inst.memory_min.as_deref().unwrap_or("512M");
    let memory_max = inst.memory_max.as_deref().unwrap_or("2G");
    let java_path = inst.java_path.as_deref().unwrap_or("system");
    let jvm_args = if inst.jvm_args.is_empty() {
        "none".to_string()
    } else {
        inst.jvm_args.join(" ")
    };
    let resolution = inst
        .resolution
        .map(|(w, h)| format!("{}x{}", w, h))
        .unwrap_or_else(|| "default".to_string());

    let lines = vec![
        Line::from(vec![
            Span::styled("Memory  ", label_style),
            Span::styled(format!("{memory_min} - {memory_max}"), value_style),
        ]),
        Line::from(vec![
            Span::styled("Java    ", label_style),
            Span::styled(java_path, value_style),
        ]),
        Line::from(vec![
            Span::styled("JVM     ", label_style),
            Span::styled(jvm_args, dim_style),
        ]),
        Line::from(vec![
            Span::styled("Display ", label_style),
            Span::styled(resolution, value_style),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines), inner);
}

pub enum SettingsAction {
    None,
    EditInstance(std::path::PathBuf),
    EditGlobal(std::path::PathBuf),
    ToggleDesktop,
}

pub fn handle_key(
    key_event: &crossterm::event::KeyEvent,
    instance: Option<&InstanceConfig>,
    instances_dir: &std::path::Path,
) -> SettingsAction {
    match key_event.code {
        crossterm::event::KeyCode::Char('e') => {
            if let Some(inst) = instance {
                let path = instances_dir.join(&inst.name).join("instance.json");
                SettingsAction::EditInstance(path)
            } else {
                SettingsAction::None
            }
        }
        crossterm::event::KeyCode::Char('g') => {
            let path = crate::config::get_config_path().join("config.toml");
            SettingsAction::EditGlobal(path)
        }
        crossterm::event::KeyCode::Char('d') => SettingsAction::ToggleDesktop,
        _ => SettingsAction::None,
    }
}
