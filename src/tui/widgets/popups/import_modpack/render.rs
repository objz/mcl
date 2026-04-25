// rendering for the modpack import wizard. same pattern as new_instance:
// snapshot the state, pick the right step renderer, done.

use super::super::base::PopupFrame;
use super::super::new_instance::LoadState;
use super::state::{IMPORT_STATE, ImportStep, ImportWizardState};
use crate::config::theme::THEME;
use crate::tui::app::FocusedArea;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph, StatefulWidget, Widget, Wrap},
};

pub fn render(frame: &mut Frame, area: Rect, _focused: FocusedArea) {
    let snapshot = match IMPORT_STATE.lock() {
        Ok(state) => state.clone(),
        Err(e) => {
            tracing::error!("Import state lock poisoned: {}", e);
            ImportWizardState::default()
        }
    };

    let keybinds = step_keybinds(&snapshot);

    let search_line = if snapshot.step == ImportStep::Version {
        snapshot.version_search.title_line()
    } else {
        None
    };

    let theme = THEME.as_ref();
    let popup = PopupFrame {
        title: wizard_title(&snapshot),
        border_color: theme.text_dim(),
        bg: Some(theme.surface()),
        keybinds: Some(keybinds),
        search_line,
        content: Box::new(move |popup_area, buf| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(1)])
                .split(popup_area);

            match snapshot.step {
                ImportStep::Input => render_input_step(&snapshot, chunks[0], buf),
                ImportStep::Fetching => render_fetching_step(chunks[0], buf),
                ImportStep::Version => render_version_step(&snapshot, chunks[0], buf),
                ImportStep::Confirm => render_confirm_step(&snapshot, chunks[0], buf),
            }
        }),
    };

    frame.render_widget(popup, area);
}

pub fn popup_rect(frame_area: Rect) -> Rect {
    let w = Constraint::Percentage(50);
    let step = match IMPORT_STATE.lock() {
        Ok(s) => s.step.clone(),
        Err(_) => ImportStep::Input,
    };

    match step {
        ImportStep::Input => {
            let h = 8u16.min(frame_area.height.saturating_sub(4));
            frame_area.centered(w, Constraint::Length(h))
        }
        ImportStep::Fetching => {
            let h = 5u16.min(frame_area.height.saturating_sub(4));
            frame_area.centered(w, Constraint::Length(h))
        }
        ImportStep::Version => {
            let h = (frame_area.height * 2 / 3)
                .max(10)
                .min(frame_area.height.saturating_sub(4));
            frame_area.centered(w, Constraint::Length(h))
        }
        ImportStep::Confirm => {
            let h = 10u16.min(frame_area.height.saturating_sub(4));
            frame_area.centered(w, Constraint::Length(h))
        }
    }
}

fn render_input_step(state: &ImportWizardState, area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let theme = THEME.as_ref();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    let input_line = if state.input.is_empty() {
        Line::from(vec![
            Span::styled(
                "URL, slug, or pack file path...",
                Style::default().fg(theme.text_dim()),
            ),
            Span::styled(
                "\u{2588}",
                Style::default()
                    .fg(theme.text_dim())
                    .add_modifier(Modifier::SLOW_BLINK),
            ),
        ])
    } else {
        Line::from(vec![
            Span::styled(state.input.clone(), Style::default().fg(theme.text())),
            Span::styled(
                "\u{2588}",
                Style::default()
                    .fg(theme.text_dim())
                    .add_modifier(Modifier::SLOW_BLINK),
            ),
        ])
    };
    Paragraph::new(input_line).render(chunks[0], buf);
}

fn render_fetching_step(area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let theme = THEME.as_ref();
    Paragraph::new("Fetching modpack info...")
        .style(Style::default().fg(theme.text_dim()))
        .render(area, buf);
}

fn render_version_step(state: &ImportWizardState, area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let theme = THEME.as_ref();
    match &state.versions {
        LoadState::Idle | LoadState::Loading => {
            Paragraph::new("Loading versions...")
                .style(Style::default().fg(theme.text_dim()))
                .render(area, buf);
        }
        LoadState::Error(message) => {
            Paragraph::new(message.as_str())
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(theme.error()))
                .render(area, buf);
        }
        LoadState::Loaded(_) => {
            let items: Vec<ListItem> = super::state::visible_versions(state)
                .into_iter()
                .map(|version| {
                    let game_ver = version.game_versions.first().cloned().unwrap_or_default();
                    let loader = version.loaders.first().cloned().unwrap_or_default();
                    ListItem::new(Line::from(Span::styled(
                        format!("{}  {}  {}", version.version_number, game_ver, loader),
                        Style::default().fg(theme.text()),
                    )))
                })
                .collect();

            let list = List::new(items)
                .highlight_style(
                    Style::default()
                        .fg(theme.accent())
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("\u{25b6} ");

            let mut list_state = ListState::default().with_selected(Some(state.version_idx));
            StatefulWidget::render(list, area, buf, &mut list_state);
        }
    }
}

fn render_confirm_step(state: &ImportWizardState, area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let theme = THEME.as_ref();
    let summary = match &state.summary {
        Some(s) => s,
        None => {
            Paragraph::new("No summary available")
                .style(Style::default().fg(theme.text_dim()))
                .render(area, buf);
            return;
        }
    };

    let label_style = Style::default().fg(theme.text_dim());

    let loader_display = if let Some(ref lv) = summary.loader_version {
        format!("{} {}", summary.loader, lv)
    } else {
        summary.loader.to_string()
    };

    Paragraph::new(vec![
        Line::from(vec![
            Span::styled("Name: ", label_style),
            Span::raw(summary.name.clone()),
        ]),
        Line::from(vec![
            Span::styled("Pack Version: ", label_style),
            Span::raw(summary.pack_version.clone()),
        ]),
        Line::from(vec![
            Span::styled("MC Version: ", label_style),
            Span::raw(summary.game_version.clone()),
        ]),
        Line::from(vec![
            Span::styled("Loader: ", label_style),
            Span::raw(loader_display),
        ]),
        Line::from(vec![
            Span::styled("Mods: ", label_style),
            Span::raw(format!("{} files", summary.mod_count)),
        ]),
        Line::from(vec![
            Span::styled("Overrides: ", label_style),
            Span::raw(format!("{} files", summary.override_count)),
        ]),
    ])
    .style(Style::default().fg(theme.text()))
    .wrap(Wrap { trim: true })
    .render(area, buf);
}

fn wizard_title(_state: &ImportWizardState) -> Line<'static> {
    use crate::tui::widgets::styled_title;
    styled_title("Import Modpack", false)
}

fn step_keybinds(state: &ImportWizardState) -> Line<'static> {
    use super::super::keybind_line;
    match state.step {
        ImportStep::Input => keybind_line(&[("Enter", " fetch")]),
        ImportStep::Fetching => keybind_line(&[("Esc", " cancel")]),
        ImportStep::Version => {
            keybind_line(&[("/", " search"), ("h", " back"), ("Enter", " select")])
        }
        ImportStep::Confirm => keybind_line(&[("h", " back"), ("Enter", " import")]),
    }
}
