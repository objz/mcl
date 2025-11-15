use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Tabs},
    Frame,
};
use throbber_widgets_tui::{Throbber, ThrobberState};

use crate::tui::layout::FocusedArea;
use crate::tui::theme::THEME;

use super::styled_title;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ContentTab {
    #[default]
    Mods,
    ResourcePacks,
    Shaders,
    Screenshots,
    Worlds,
    Logs,
}

impl ContentTab {
    const ALL: &'static [ContentTab] = &[
        ContentTab::Mods,
        ContentTab::ResourcePacks,
        ContentTab::Shaders,
        ContentTab::Screenshots,
        ContentTab::Worlds,
        ContentTab::Logs,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ContentTab::Mods => "Mods",
            ContentTab::ResourcePacks => "Resource Packs",
            ContentTab::Shaders => "Shaders",
            ContentTab::Screenshots => "Screenshots",
            ContentTab::Worlds => "Worlds",
            ContentTab::Logs => "Logs",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }

    pub fn next(self) -> Self {
        Self::ALL[(self.index() + 1) % Self::ALL.len()]
    }

    pub fn previous(self) -> Self {
        let idx = self.index();
        Self::ALL[if idx == 0 {
            Self::ALL.len() - 1
        } else {
            idx - 1
        }]
    }
}

pub fn render(
    frame: &mut Frame,
    area: Rect,
    focused: FocusedArea,
    tab: ContentTab,
    instance: Option<&crate::instance::InstanceConfig>,
    mods_state: &mut super::content_list::ContentListState,
    resource_packs_state: &mut super::content_list::ContentListState,
    shaders_state: &mut super::content_list::ContentListState,
    instances_dir: &std::path::Path,
) {
    let is_focused = focused == FocusedArea::Content;

    let border_color = if is_focused {
        THEME.colors.border_focused
    } else {
        THEME.colors.border_unfocused
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [tabs_area, content_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);

    let tab_titles: Vec<&str> = ContentTab::ALL.iter().map(|t| t.label()).collect();
    let tabs = Tabs::new(tab_titles)
        .select(tab.index())
        .highlight_style(
            Style::default()
                .fg(THEME.colors.accent)
                .add_modifier(Modifier::BOLD),
        )
        .divider(Span::styled(
            " │ ",
            Style::default().fg(THEME.colors.border_unfocused),
        ));

    frame.render_widget(tabs, tabs_area);

    match tab {
        ContentTab::Mods => {
            if let Some(instance) = instance {
                if mods_state.loaded_for.as_deref() != Some(instance.name.as_str()) {
                    mods_state.start_load(
                        instances_dir,
                        &instance.name,
                        crate::instance::scan_mods,
                    );
                }
                super::content_list::render(
                    frame,
                    content_area,
                    mods_state,
                    is_focused,
                    "Loading mods...",
                    "No mods installed.",
                );
            } else {
                frame.render_widget(
                    Paragraph::new("No instance selected.")
                        .style(Style::default().fg(THEME.colors.text_idle)),
                    content_area,
                );
            }
        }
        ContentTab::ResourcePacks => {
            if let Some(instance) = instance {
                if resource_packs_state.loaded_for.as_deref() != Some(instance.name.as_str()) {
                    resource_packs_state.start_load(
                        instances_dir,
                        &instance.name,
                        crate::instance::scan_resource_packs,
                    );
                }
                super::content_list::render(
                    frame,
                    content_area,
                    resource_packs_state,
                    is_focused,
                    "Loading resource packs...",
                    "No resource packs installed.",
                );
            } else {
                frame.render_widget(
                    Paragraph::new("No instance selected.")
                        .style(Style::default().fg(THEME.colors.text_idle)),
                    content_area,
                );
            }
        }
        ContentTab::Shaders => {
            if let Some(instance) = instance {
                if shaders_state.loaded_for.as_deref() != Some(instance.name.as_str()) {
                    shaders_state.start_load(
                        instances_dir,
                        &instance.name,
                        crate::instance::scan_shaders,
                    );
                }
                super::content_list::render(
                    frame,
                    content_area,
                    shaders_state,
                    is_focused,
                    "Loading shaders...",
                    "No shaders installed.",
                );
            } else {
                frame.render_widget(
                    Paragraph::new("No instance selected.")
                        .style(Style::default().fg(THEME.colors.text_idle)),
                    content_area,
                );
            }
        }
        ContentTab::Logs => {
            let lines = instance
                .map(|i| crate::instance_logs::get_all(&i.name))
                .unwrap_or_default();
            if lines.is_empty() {
                frame.render_widget(
                    Paragraph::new("No logs yet.")
                        .style(Style::default().fg(THEME.colors.text_idle)),
                    content_area,
                );
            } else {
                frame.render_widget(
                    Paragraph::new(lines.join("\n"))
                        .style(Style::default().fg(THEME.colors.text_idle))
                        .wrap(ratatui::widgets::Wrap { trim: false }),
                    content_area,
                );
            }
        }
        _ => {
            let body = match tab {
                ContentTab::Screenshots => "No screenshots.",
                ContentTab::Worlds => "No worlds saved.",
                _ => unreachable!(),
            };

            frame.render_widget(
                Paragraph::new(body).style(Style::default().fg(THEME.colors.text_idle)),
                content_area,
            );
        }
    }
}

pub fn title(
    frame: &mut Frame,
    area: Rect,
    focused: FocusedArea,
    instance: Option<&crate::instance::InstanceConfig>,
    throbber_state: &mut ThrobberState,
) {
    let color = if focused == FocusedArea::Content {
        THEME.colors.border_focused
    } else {
        THEME.colors.border_unfocused
    };

    let block = Block::default()
        .title(styled_title("Content", true))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    match instance {
        None => {
            frame.render_widget(
                Paragraph::new("No instance selected")
                    .style(Style::default().fg(THEME.colors.text_idle)),
                inner,
            );
        }
        Some(inst) => {
            let [left_area, right_area] =
                Layout::horizontal([Constraint::Min(0), Constraint::Length(32)]).areas(inner);

            use crate::running::RunState;
            let run_state = crate::running::get(&inst.name);

            match run_state {
                Some(RunState::Running) | Some(RunState::Starting) => {
                    let throbber = Throbber::default()
                        .label(inst.name.as_str())
                        .style(
                            Style::default()
                                .fg(THEME.colors.foreground)
                                .add_modifier(Modifier::BOLD),
                        )
                        .throbber_style(
                            Style::default()
                                .fg(THEME.colors.success)
                                .add_modifier(Modifier::BOLD),
                        )
                        .throbber_set(throbber_widgets_tui::BRAILLE_EIGHT_DOUBLE)
                        .use_type(throbber_widgets_tui::WhichUse::Spin);
                    frame.render_stateful_widget(throbber, left_area, throbber_state);
                }
                Some(RunState::Crashed(_)) => {
                    frame.render_widget(
                        Paragraph::new(Line::from(vec![
                            Span::styled(
                                "\u{2717} ",
                                Style::default()
                                    .fg(THEME.colors.error)
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                inst.name.as_str(),
                                Style::default()
                                    .fg(THEME.colors.foreground)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ])),
                        left_area,
                    );
                }
                None => {
                    frame.render_widget(
                        Paragraph::new(Span::styled(
                            inst.name.as_str(),
                            Style::default()
                                .fg(THEME.colors.foreground)
                                .add_modifier(Modifier::BOLD),
                        )),
                        left_area,
                    );
                }
            }

            let loader_str = match &inst.loader_version {
                Some(lv) => format!("{} \u{00b7} {} {}", inst.game_version, inst.loader, lv),
                None => format!("{} \u{00b7} {}", inst.game_version, inst.loader),
            };
            frame.render_widget(
                Paragraph::new(loader_str)
                    .style(Style::default().fg(THEME.colors.border_focused))
                    .alignment(Alignment::Right),
                right_area,
            );
        }
    }
}
