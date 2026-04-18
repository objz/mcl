use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use throbber_widgets_tui::{Throbber, ThrobberState};

use crate::tui::app::FocusedArea;
use crate::config::theme::{THEME, BORDER_STYLE};

use crate::tui::widgets::styled_title;

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

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    area: Rect,
    focused: FocusedArea,
    tab: ContentTab,
    instance: Option<&crate::instance::InstanceConfig>,
    mods_state: &mut super::list::ContentListState,
    resource_packs_state: &mut super::list::ContentListState,
    shaders_state: &mut super::list::ContentListState,
    worlds_state: &mut super::list::ContentListState,
    screenshots_state: &mut crate::tui::widgets::screenshots_grid::ScreenshotsState,
    logs_state: &mut crate::tui::widgets::logs_viewer::LogsState,
    instances_dir: &std::path::Path,
) {
    let theme = THEME.as_ref();
    let is_focused = focused == FocusedArea::Content;

    let border_color = if is_focused {
        theme.accent()
    } else {
        theme.border()
    };

    let tab_titles: Vec<Span> = ContentTab::ALL
        .iter()
        .enumerate()
        .flat_map(|(i, t)| {
            let mut spans = Vec::new();
            if i > 0 {
                spans.push(Span::styled(
                    "\u{2022}",
                    Style::default().fg(theme.text_dim()),
                ));
            }
            if i == tab.index() {
                let style = Style::default()
                    .fg(theme.accent())
                    .add_modifier(Modifier::BOLD);
                spans.push(Span::styled(format!(" {} ", t.label()), style));
            } else {
                spans.push(Span::styled(
                    format!(" {} ", t.label()),
                    Style::default().fg(theme.text()),
                ));
            }
            spans
        })
        .collect();

    let search_line = match tab {
        ContentTab::Mods => mods_state.search.title_line(),
        ContentTab::ResourcePacks => resource_packs_state.search.title_line(),
        ContentTab::Shaders => shaders_state.search.title_line(),
        ContentTab::Worlds => worlds_state.search.title_line(),
        ContentTab::Screenshots => screenshots_state.search.title_line(),
        ContentTab::Logs => {
            if logs_state.viewer_focused {
                logs_state.viewer_search.title_line()
            } else {
                logs_state.search.title_line()
            }
        }
    };

    let mut block = Block::default()
        .title_top(Line::from(tab_titles))
        .borders(Borders::ALL)
        .border_type(BORDER_STYLE.to_border_type())
        .border_style(Style::default().fg(border_color));

    if let Some(sl) = search_line {
        block = block.title_top(sl);
    }

    let kb: Option<&[(&str, &str)]> = if is_focused {
        Some(match tab {
            ContentTab::Mods | ContentTab::ResourcePacks | ContentTab::Shaders => &[
                ("j/k", " navigate"),
                ("⏎", " toggle"),
                ("Shift+⏎", " open dir"),
                ("h/l", " tabs"),
                ("/", " search"),
            ],
            ContentTab::Worlds => &[
                ("j/k", " navigate"),
                ("Shift+⏎", " open dir"),
                ("h/l", " tabs"),
                ("/", " search"),
            ],
            ContentTab::Screenshots => &[
                ("Shift+HJKL", " grid"),
                ("⏎", " open"),
                ("Shift+⏎", " open dir"),
                ("h/l", " tabs"),
                ("/", " search"),
            ],
            ContentTab::Logs => {
                if logs_state.viewer_focused {
                    &[
                        ("j/k", " scroll"),
                        ("g/G", " top/bottom"),
                        ("Esc", " back"),
                        ("/", " search"),
                    ]
                } else {
                    &[
                        ("j/k", " navigate"),
                        ("⏎", " view"),
                        ("h/l", " tabs"),
                        ("/", " search"),
                    ]
                }
            }
        })
    } else if focused == FocusedArea::Instances {
        Some(&[
            ("l", " launch"),
            ("⏎", " content"),
            ("Shift+⏎", " open dir"),
            ("Esc", " kill"),
            ("a", " add"),
            ("i", " import"),
            ("d", " delete"),
            ("r", " rename"),
            ("/", " search"),
        ])
    } else {
        None
    };

    if let Some(kb) = kb {
        let lines = crate::tui::widgets::popups::keybind_lines_wrapped(kb, area.width.saturating_sub(2));
        for line in lines {
            block = block.title_bottom(line);
        }
    }

    let content_area = block.inner(area);
    frame.render_widget(block, area);

    match tab {
        ContentTab::Mods => {
            if let Some(instance) = instance {
                if mods_state.loaded_for.as_deref() != Some(instance.name.as_str()) {
                    mods_state.start_load(
                        instances_dir,
                        &instance.name,
                        crate::instance::scan_mods,
                    );
                    mods_state.watch_dir(instances_dir.join(&instance.name).join(".minecraft").join("mods"));
                }
                super::list::render(
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
                        .style(Style::default().fg(theme.text_dim())),
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
                    resource_packs_state.watch_dir(instances_dir.join(&instance.name).join(".minecraft").join("resourcepacks"));
                }
                super::list::render(
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
                        .style(Style::default().fg(theme.text_dim())),
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
                    shaders_state.watch_dir(instances_dir.join(&instance.name).join(".minecraft").join("shaderpacks"));
                }
                super::list::render(
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
                        .style(Style::default().fg(theme.text_dim())),
                    content_area,
                );
            }
        }
        ContentTab::Logs => {
            if let Some(instance) = instance {
                if logs_state.loaded_for.as_deref() != Some(instance.name.as_str()) {
                    logs_state.start_load(instances_dir, &instance.name);
                }
                crate::tui::widgets::logs_viewer::render(frame, content_area, logs_state, is_focused);
            } else {
                frame.render_widget(
                    Paragraph::new("No instance selected.")
                        .style(Style::default().fg(theme.text_dim())),
                    content_area,
                );
            }
        }
        ContentTab::Screenshots => {
            if let Some(instance) = instance {
                if screenshots_state.loaded_for.as_deref() != Some(instance.name.as_str()) {
                    screenshots_state.start_load(instances_dir, &instance.name);
                }
                crate::tui::widgets::screenshots_grid::render(frame, content_area, screenshots_state, is_focused);
            } else {
                frame.render_widget(
                    Paragraph::new("No instance selected.")
                        .style(Style::default().fg(theme.text_dim())),
                    content_area,
                );
            }
        }
        ContentTab::Worlds => {
            if let Some(instance) = instance {
                if worlds_state.loaded_for.as_deref() != Some(instance.name.as_str()) {
                    worlds_state.start_load(
                        instances_dir,
                        &instance.name,
                        crate::instance::scan_worlds,
                    );
                    worlds_state.watch_dir(instances_dir.join(&instance.name).join(".minecraft").join("saves"));
                }
                super::list::render(
                    frame,
                    content_area,
                    worlds_state,
                    is_focused,
                    "Loading worlds...",
                    "No worlds saved.",
                );
            } else {
                frame.render_widget(
                    Paragraph::new("No instance selected.")
                        .style(Style::default().fg(theme.text_dim())),
                    content_area,
                );
            }
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
    let theme = THEME.as_ref();
    let color = if focused == FocusedArea::Content {
        theme.accent()
    } else {
        theme.border()
    };

    let block = Block::default()
        .title(styled_title("Content", true))
        .borders(Borders::ALL)
        .border_type(BORDER_STYLE.to_border_type())
        .border_style(Style::default().fg(color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    match instance {
        None => {
            frame.render_widget(
                Paragraph::new("No instance selected")
                    .style(Style::default().fg(theme.text_dim())),
                inner,
            );
        }
        Some(inst) => {
            let [left_area, right_area] =
                Layout::horizontal([Constraint::Min(0), Constraint::Length(32)]).areas(inner);

            use crate::running::RunState;
            let run_state = crate::running::get(&inst.name);

            match run_state {
                Some(RunState::Authenticating)
                | Some(RunState::Running)
                | Some(RunState::Starting) => {
                    let throbber = Throbber::default()
                        .label(inst.name.as_str())
                        .style(
                            Style::default()
                                .fg(theme.text())
                                .add_modifier(Modifier::BOLD),
                        )
                        .throbber_style(
                            Style::default()
                                .fg(theme.success())
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
                                    .fg(theme.error())
                                    .add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                inst.name.as_str(),
                                Style::default()
                                    .fg(theme.text())
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
                                .fg(theme.text())
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
                    .style(Style::default().fg(theme.text_dim()))
                    .alignment(Alignment::Right),
                right_area,
            );
        }
    }
}
