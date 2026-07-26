// the outer frame for the content area: tab bar, keybind footer,
// and dispatching render calls to the active tab's widget.
// also renders the instance name/version header with run state indicators.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, ListItem, Paragraph, Widget, Wrap},
};
use throbber_widgets_tui::{Throbber, ThrobberState};

use crate::config::theme::{BORDER_STYLE, THEME};
use crate::tui::app::FocusedArea;
use crate::tui::widgets::content::{ContentMode, DiscoveryState};

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

    const DISCOVERY: &'static [ContentTab] = &[
        ContentTab::Mods,
        ContentTab::ResourcePacks,
        ContentTab::Shaders,
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

    pub fn next_for_mode(self, mode: ContentMode) -> Self {
        cycle_tab(self, visible_tabs(mode), true)
    }

    pub fn previous_for_mode(self, mode: ContentMode) -> Self {
        cycle_tab(self, visible_tabs(mode), false)
    }
}

fn visible_tabs(mode: ContentMode) -> &'static [ContentTab] {
    match mode {
        ContentMode::Installed => ContentTab::ALL,
        ContentMode::Discover => ContentTab::DISCOVERY,
    }
}

fn mode_label(mode: ContentMode) -> String {
    format!(" {} ", mode.label())
}

fn cycle_tab(current: ContentTab, tabs: &[ContentTab], forward: bool) -> ContentTab {
    let index = tabs.iter().position(|tab| *tab == current).unwrap_or(0);
    if forward {
        tabs[(index + 1) % tabs.len()]
    } else {
        tabs[if index == 0 {
            tabs.len() - 1
        } else {
            index - 1
        }]
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame,
    area: Rect,
    focused: FocusedArea,
    tab: ContentTab,
    mode: ContentMode,
    instance: Option<&crate::instance::InstanceConfig>,
    mods_state: &mut super::list::ContentListState,
    mods_discovery_state: &mut DiscoveryState,
    resource_packs_state: &mut super::list::ContentListState,
    resource_packs_discovery_state: &mut DiscoveryState,
    shaders_state: &mut super::list::ContentListState,
    shaders_discovery_state: &mut DiscoveryState,
    worlds_state: &mut super::list::ContentListState,
    screenshots_state: &mut crate::tui::widgets::screenshots_grid::ScreenshotsState,
    logs_state: &mut crate::tui::widgets::logs_viewer::LogsState,
    instances_dir: &std::path::Path,
    picker: &ratatui_image::picker::Picker,
) {
    let theme = THEME.as_ref();
    let is_focused = focused == FocusedArea::Content;

    let border_color = if is_focused {
        theme.accent()
    } else {
        theme.border()
    };

    let tabs = visible_tabs(mode);
    let tab_titles: Vec<Span> = tabs
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
            if tabs.get(i) == Some(&tab) {
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
        ContentTab::Mods if mode == ContentMode::Discover => {
            mods_discovery_state.search.title_line()
        }
        ContentTab::Mods => mods_state.search.title_line(),
        ContentTab::ResourcePacks if mode == ContentMode::Discover => {
            resource_packs_discovery_state.search.title_line()
        }
        ContentTab::ResourcePacks => resource_packs_state.search.title_line(),
        ContentTab::Shaders if mode == ContentMode::Discover => {
            shaders_discovery_state.search.title_line()
        }
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

    let mode_background = match mode {
        ContentMode::Installed => theme.success(),
        ContentMode::Discover => theme.info(),
    };
    let mut content_titles = vec![
        Span::styled(
            mode_label(mode),
            Style::default()
                .fg(theme.background())
                .bg(mode_background)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
    ];
    content_titles.extend(tab_titles);

    let mut block = Block::default()
        .title_top(Line::from(content_titles))
        .borders(Borders::ALL)
        .border_type(BORDER_STYLE.to_border_type())
        .border_style(Style::default().fg(border_color));

    if let Some(sl) = search_line {
        block = block.title_top(sl);
    }

    let discovery_can_delete = match tab {
        ContentTab::Mods => mods_discovery_state.selected_is_installed(),
        ContentTab::ResourcePacks => resource_packs_discovery_state.selected_is_installed(),
        ContentTab::Shaders => shaders_discovery_state.selected_is_installed(),
        _ => false,
    };
    let discovery_page_open = match tab {
        ContentTab::Mods => mods_discovery_state.project_page_open(),
        ContentTab::ResourcePacks => resource_packs_discovery_state.project_page_open(),
        ContentTab::Shaders => shaders_discovery_state.project_page_open(),
        _ => false,
    };

    // keybinds change depending on which tab is active and whether
    // the content panel or instances panel has focus
    let kb: Option<&[(&str, &str)]> = if is_focused {
        Some(match (mode, tab) {
            (ContentMode::Discover, _) if discovery_page_open => {
                &[("j/k", " scroll"), ("g/G", " top/bottom"), ("h", " back")]
            }
            (ContentMode::Discover, _) if discovery_can_delete => &[
                ("j/k", " navigate"),
                ("Enter", " view"),
                ("i", " versions"),
                ("d", " delete"),
                ("h/l", " tabs"),
                ("/", " search"),
                ("Tab", " installed"),
            ],
            (ContentMode::Discover, _) => &[
                ("j/k", " navigate"),
                ("Enter", " view"),
                ("i", " versions"),
                ("h/l", " tabs"),
                ("/", " search"),
                ("Tab", " installed"),
            ],
            (ContentMode::Installed, ContentTab::Mods)
            | (ContentMode::Installed, ContentTab::ResourcePacks)
            | (ContentMode::Installed, ContentTab::Shaders) => &[
                ("j/k", " navigate"),
                ("⏎", " toggle"),
                ("d", " delete"),
                ("Shift+⏎", " open dir"),
                ("h/l", " tabs"),
                ("/", " search"),
                ("Tab", " discover"),
            ],
            (ContentMode::Installed, ContentTab::Worlds) => &[
                ("j/k", " navigate"),
                ("d", " delete"),
                ("Shift+⏎", " open dir"),
                ("h/l", " tabs"),
                ("/", " search"),
                ("Tab", " discover"),
            ],
            (ContentMode::Installed, ContentTab::Screenshots) => &[
                ("Shift+HJKL", " grid"),
                ("⏎", " open"),
                ("d", " delete"),
                ("Shift+⏎", " open dir"),
                ("h/l", " tabs"),
                ("/", " search"),
                ("Tab", " discover"),
            ],
            (ContentMode::Installed, ContentTab::Logs) => {
                if logs_state.viewer_focused {
                    &[
                        ("j/k", " scroll"),
                        ("g/G", " top/bottom"),
                        ("d", " delete"),
                        ("Esc", " back"),
                        ("/", " search"),
                        ("Tab", " discover"),
                    ]
                } else {
                    &[
                        ("j/k", " navigate"),
                        ("⏎", " view"),
                        ("d", " delete"),
                        ("h/l", " tabs"),
                        ("/", " search"),
                        ("Tab", " discover"),
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
        let lines =
            crate::tui::widgets::popups::keybind_lines_wrapped(kb, area.width.saturating_sub(2));
        for line in lines {
            block = block.title_bottom(line);
        }
    }

    let content_area = block.inner(area);
    frame.render_widget(block, area);

    // lazy-load: only scan when switching to an instance that hasn't been loaded yet
    match tab {
        ContentTab::Mods => {
            if let Some(instance) = instance {
                if mods_state.loaded_for.as_deref() != Some(instance.name.as_str()) {
                    let content_dir = instances_dir
                        .join(&instance.name)
                        .join(crate::storage::MINECRAFT_DIR_NAME)
                        .join("mods");
                    mods_state.start_load(
                        &content_dir,
                        &instance.name,
                        crate::instance::scan_one_mod,
                        ".jar",
                    );
                    mods_state.watch_dir(content_dir);
                }
                if mode == ContentMode::Discover {
                    render_discovery(
                        frame,
                        content_area,
                        mods_discovery_state,
                        is_focused,
                        "Searching Modrinth...",
                        picker,
                    );
                } else {
                    super::list::render(
                        frame,
                        content_area,
                        mods_state,
                        is_focused,
                        "Loading mods...",
                        "No mods installed.",
                        picker,
                    );
                }
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
                    let content_dir = instances_dir
                        .join(&instance.name)
                        .join(crate::storage::MINECRAFT_DIR_NAME)
                        .join("resourcepacks");
                    resource_packs_state.start_load(
                        &content_dir,
                        &instance.name,
                        crate::instance::scan_one_resource_pack,
                        ".zip",
                    );
                    resource_packs_state.watch_dir(content_dir);
                }
                if mode == ContentMode::Discover {
                    render_discovery(
                        frame,
                        content_area,
                        resource_packs_discovery_state,
                        is_focused,
                        "Searching Modrinth...",
                        picker,
                    );
                } else {
                    super::list::render(
                        frame,
                        content_area,
                        resource_packs_state,
                        is_focused,
                        "Loading resource packs...",
                        "No resource packs installed.",
                        picker,
                    );
                }
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
                    let content_dir = instances_dir
                        .join(&instance.name)
                        .join(crate::storage::MINECRAFT_DIR_NAME)
                        .join("shaderpacks");
                    shaders_state.start_load(
                        &content_dir,
                        &instance.name,
                        crate::instance::scan_one_shader,
                        ".zip",
                    );
                    shaders_state.watch_dir(content_dir);
                }
                if mode == ContentMode::Discover {
                    render_discovery(
                        frame,
                        content_area,
                        shaders_discovery_state,
                        is_focused,
                        "Searching Modrinth...",
                        picker,
                    );
                } else {
                    super::list::render(
                        frame,
                        content_area,
                        shaders_state,
                        is_focused,
                        "Loading shaders...",
                        "No shaders installed.",
                        picker,
                    );
                }
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
                crate::tui::widgets::logs_viewer::render(
                    frame,
                    content_area,
                    logs_state,
                    is_focused,
                );
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
                crate::tui::widgets::screenshots_grid::render(
                    frame,
                    content_area,
                    screenshots_state,
                    is_focused,
                );
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
                    let content_dir = instances_dir
                        .join(&instance.name)
                        .join(crate::storage::MINECRAFT_DIR_NAME)
                        .join("saves");
                    worlds_state.start_load(
                        &content_dir,
                        &instance.name,
                        crate::instance::scan_one_world,
                        "",
                    );
                    worlds_state.watch_dir(content_dir);
                }
                super::list::render(
                    frame,
                    content_area,
                    worlds_state,
                    is_focused,
                    "Loading worlds...",
                    "No worlds saved.",
                    picker,
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

fn render_discovery(
    frame: &mut Frame,
    area: Rect,
    state: &mut DiscoveryState,
    is_focused: bool,
    loading_text: &str,
    picker: &ratatui_image::picker::Picker,
) {
    state.set_viewport_rows(area.height);
    if let Some(page) = state.project_page.as_mut() {
        if let Some(error) = page.error.as_deref() {
            frame.render_widget(
                Paragraph::new(error)
                    .style(Style::default().fg(THEME.as_ref().error()))
                    .wrap(Wrap { trim: true }),
                area,
            );
        } else if let Some(document) = page.document.as_mut() {
            page.max_scroll = crate::tui::widgets::markdown::render(
                frame,
                area,
                document,
                &mut page.scroll,
                picker,
            );
        } else {
            frame.render_widget(
                Paragraph::new(format!("Loading {}...", page.title))
                    .style(Style::default().fg(THEME.as_ref().text_dim())),
                area,
            );
        }
        return;
    }
    let empty_text = state.empty_text().to_string();
    super::list::render(
        frame,
        area,
        &mut state.list,
        is_focused,
        loading_text,
        &empty_text,
        picker,
    );
    if state.version_popup.is_some() {
        render_version_popup(frame, area, state);
    }
}

fn render_version_popup(frame: &mut Frame, area: Rect, state: &DiscoveryState) {
    let Some(popup) = state.version_popup.as_ref() else {
        return;
    };
    let desired_height = version_popup_height(
        popup.versions.len(),
        popup.confirming,
        popup.loading || popup.installing,
        popup.error.is_some(),
    );
    let popup_area = area.centered(
        Constraint::Percentage(50),
        Constraint::Length(desired_height.min(area.height.saturating_sub(2))),
    );
    let theme = THEME.as_ref();
    let title = popup.title();
    let loading = popup.loading;
    let installing = popup.installing;
    let confirming = popup.confirming;
    let error = popup.error.clone();
    let selected = popup.selected;
    let selected_version = popup
        .versions
        .get(selected)
        .map(|version| version.version_number.clone())
        .unwrap_or_default();
    let minecraft_versions = popup
        .versions
        .get(selected)
        .map(|version| confirmation_values(&version.game_versions))
        .unwrap_or_else(|| "Unknown".to_owned());
    let loaders = popup
        .versions
        .get(selected)
        .map(|version| confirmation_loaders(&version.loaders))
        .unwrap_or_else(|| "Unknown".to_owned());
    let release_date = popup
        .versions
        .get(selected)
        .map(|version| confirmation_release_date(&version.date_published))
        .unwrap_or_else(|| "Unknown".to_owned());
    let replacing = popup.installed_path.is_some();
    let items = popup
        .versions
        .iter()
        .map(|version| {
            ListItem::new(discovery_version_label(version)).style(Style::default().fg(theme.text()))
        })
        .collect::<Vec<_>>();
    let keybinds = if confirming {
        crate::tui::widgets::popups::keybind_line(&[
            ("h", " back"),
            ("Enter", if replacing { " change" } else { " install" }),
        ])
    } else {
        crate::tui::widgets::popups::keybind_line(&[
            ("j/k", " navigate"),
            ("Enter", " continue"),
            ("Esc", " close"),
        ])
    };

    let popup = crate::tui::widgets::popups::base::PopupFrame {
        title: styled_title(&title, false),
        border_color: theme.accent(),
        bg: Some(theme.surface()),
        keybinds: Some(keybinds),
        search_line: None,
        content: Box::new(move |area, buffer| {
            if installing {
                Paragraph::new("Installing...")
                    .style(Style::default().fg(THEME.as_ref().text_dim()))
                    .render(area, buffer);
            } else if loading {
                Paragraph::new("Loading compatible versions...")
                    .style(Style::default().fg(THEME.as_ref().text_dim()))
                    .render(area, buffer);
            } else if let Some(error) = &error {
                Paragraph::new(error.as_str())
                    .style(Style::default().fg(THEME.as_ref().error()))
                    .wrap(Wrap { trim: true })
                    .render(area, buffer);
            } else if confirming {
                crate::tui::widgets::popups::base::render_summary(
                    &[
                        ("Version", selected_version.as_str()),
                        ("Minecraft", minecraft_versions.as_str()),
                        ("Loader", loaders.as_str()),
                        ("Released", release_date.as_str()),
                    ],
                    area,
                    buffer,
                );
            } else if items.is_empty() {
                Paragraph::new("No compatible versions found.")
                    .style(Style::default().fg(THEME.as_ref().text_dim()))
                    .render(area, buffer);
            } else {
                crate::tui::widgets::popups::new_instance::render_select_list(
                    items.clone(),
                    selected,
                    area,
                    buffer,
                );
            }
        }),
    };
    frame.render_widget(popup, popup_area);
}

fn version_popup_height(version_count: usize, confirming: bool, compact: bool, error: bool) -> u16 {
    if confirming || error {
        8
    } else if compact {
        5
    } else {
        (version_count as u16).saturating_add(2).clamp(6, 18)
    }
}

fn confirmation_values(values: &[String]) -> String {
    if values.is_empty() {
        "Unknown".to_owned()
    } else {
        values.join(", ")
    }
}

fn confirmation_loaders(loaders: &[String]) -> String {
    let loaders = loaders
        .iter()
        .map(|loader| match loader.as_str() {
            "fabric" => "Fabric",
            "forge" => "Forge",
            "neoforge" => "NeoForge",
            "quilt" => "Quilt",
            "minecraft" => "Minecraft",
            other => other,
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    confirmation_values(&loaders)
}

fn confirmation_release_date(value: &str) -> String {
    if value.is_empty() {
        return "Unknown".to_owned();
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .map(|date| date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|_| value.to_owned())
}

fn discovery_version_label(version: &crate::net::modrinth::VersionInfo) -> String {
    version.version_number.clone()
}

// the header bar above the content tabs, showing instance name, loader info,
// and a spinner/error indicator when the instance is running or crashed
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
                Paragraph::new("No instance selected").style(Style::default().fg(theme.text_dim())),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_labels_have_the_same_rendered_width() {
        assert_eq!(
            mode_label(ContentMode::Installed).chars().count(),
            mode_label(ContentMode::Discover).chars().count()
        );
    }

    #[test]
    fn discovery_navigation_only_cycles_downloadable_tabs() {
        assert_eq!(
            ContentTab::Shaders.next_for_mode(ContentMode::Discover),
            ContentTab::Mods
        );
        assert_eq!(
            ContentTab::Mods.previous_for_mode(ContentMode::Discover),
            ContentTab::Shaders
        );
    }

    #[test]
    fn discovery_navigation_recovers_from_hidden_local_tab() {
        assert_eq!(
            ContentTab::Logs.next_for_mode(ContentMode::Discover),
            ContentTab::ResourcePacks
        );
    }

    #[test]
    fn discovery_version_rows_only_show_the_version_number() {
        let version = crate::net::modrinth::VersionInfo {
            id: "version-id".to_owned(),
            project_id: "project-id".to_owned(),
            name: "A descriptive release title".to_owned(),
            version_number: "3.2.4-fabric-26.1".to_owned(),
            game_versions: vec![],
            loaders: vec![],
            date_published: String::new(),
            files: vec![],
        };

        assert_eq!(discovery_version_label(&version), "3.2.4-fabric-26.1");
    }

    #[test]
    fn discovery_version_popup_uses_compact_heights() {
        assert_eq!(version_popup_height(1, false, false, false), 6);
        assert_eq!(version_popup_height(100, false, false, false), 18);
        assert_eq!(version_popup_height(1, true, false, false), 8);
        assert_eq!(version_popup_height(0, false, true, false), 5);
        assert_eq!(version_popup_height(0, false, false, true), 8);
    }

    #[test]
    fn confirmation_metadata_is_human_readable() {
        assert_eq!(
            confirmation_loaders(&["fabric".to_owned(), "neoforge".to_owned()]),
            "Fabric, NeoForge"
        );
        assert_eq!(
            confirmation_release_date("2026-07-26T14:30:00Z"),
            "2026-07-26"
        );
        assert_eq!(confirmation_values(&[]), "Unknown");
    }
}
