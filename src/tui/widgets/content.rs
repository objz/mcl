use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Span,
    widgets::{Block, BorderType, Borders, Paragraph, Tabs},
    Frame,
};

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

pub fn render(frame: &mut Frame, area: Rect, focused: FocusedArea, tab: ContentTab) {
    let border_color = if focused == FocusedArea::Content {
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

    let body = match tab {
        ContentTab::Mods => "No mods installed.",
        ContentTab::ResourcePacks => "No resource packs installed.",
        ContentTab::Shaders => "No shaders installed.",
        ContentTab::Screenshots => "No screenshots.",
        ContentTab::Worlds => "No worlds saved.",
        ContentTab::Logs => "No logs yet.",
    };

    frame.render_widget(
        Paragraph::new(body).style(Style::default().fg(THEME.colors.text_idle)),
        content_area,
    );
}

pub fn title(
    frame: &mut Frame,
    area: Rect,
    focused: FocusedArea,
    instance: Option<&crate::instance::InstanceConfig>,
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

            frame.render_widget(
                Paragraph::new(inst.name.as_str()).style(
                    Style::default()
                        .fg(THEME.colors.foreground)
                        .add_modifier(Modifier::BOLD),
                ),
                left_area,
            );

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
