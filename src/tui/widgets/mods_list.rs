use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::Paragraph,
    Frame,
};
use tui_widget_list::{ListBuilder, ListState as TuiListState, ListView};

use crate::instance::mods::{scan_mods, toggle_mod, ModEntry};
use crate::tui::theme::THEME;

type IconCell = (u8, u8, u8, u8, u8, u8);

pub struct ModsState {
    pub mods: Vec<ModEntry>,
    pub list_state: TuiListState,
    pub loaded_for: Option<String>,
}

impl Default for ModsState {
    fn default() -> Self {
        Self {
            mods: Vec::new(),
            list_state: TuiListState::default(),
            loaded_for: None,
        }
    }
}

impl ModsState {
    pub fn load_mods(&mut self, instances_dir: &Path, instance_name: &str) {
        self.mods = scan_mods(instances_dir, instance_name);
        self.loaded_for = Some(instance_name.to_string());
        self.list_state = TuiListState::default();
        if !self.mods.is_empty() {
            self.list_state.selected = Some(0);
        }
    }

    pub fn toggle_selected(&mut self, instances_dir: &Path) {
        let Some(index) = self.list_state.selected else {
            return;
        };
        let Some(entry) = self.mods.get(index).cloned() else {
            return;
        };

        match toggle_mod(&entry) {
            Ok(()) => {
                let loaded_for = self.loaded_for.clone();
                if let Some(instance_name) = loaded_for.as_deref() {
                    self.load_mods(instances_dir, instance_name);
                    if !self.mods.is_empty() {
                        self.list_state.selected = Some(index.min(self.mods.len() - 1));
                    }
                }
            }
            Err(error) => {
                tracing::error!("Failed to toggle mod '{}': {}", entry.file_stem, error);
            }
        }
    }
}

/// Returns `true` if the key was consumed by the mods list.
pub fn handle_key(key_event: &KeyEvent, state: &mut ModsState, instances_dir: &Path) -> bool {
    match key_event.code {
        KeyCode::Char('j') | KeyCode::Down => {
            let count = state.mods.len();
            if count == 0 {
                return true;
            }
            let current = state.list_state.selected.unwrap_or(0);
            state.list_state.selected = Some((current + 1).min(count - 1));
            true
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let current = state.list_state.selected.unwrap_or(0);
            state.list_state.selected = Some(current.saturating_sub(1));
            true
        }
        KeyCode::Enter => {
            state.toggle_selected(instances_dir);
            true
        }
        _ => false,
    }
}

pub fn render(frame: &mut Frame, area: Rect, state: &mut ModsState) {
    if state.mods.is_empty() {
        frame.render_widget(
            Paragraph::new("No mods installed.").style(Style::default().fg(THEME.colors.text_idle)),
            area,
        );
        return;
    }

    let count = state.mods.len();
    const ITEM_HEIGHT: u16 = 2;

    let mods_snapshot: Vec<(String, String, bool, Option<Vec<Vec<IconCell>>>)> = state
        .mods
        .iter()
        .map(|mod_entry| {
            (
                mod_entry.name.clone(),
                mod_entry.description.clone(),
                mod_entry.enabled,
                mod_entry.icon_lines.clone(),
            )
        })
        .collect();

    let builder = ListBuilder::new(move |context| {
        let (name, description, enabled, icon_pixels) = &mods_snapshot[context.index];

        let (name_style, description_style, background) = if context.is_selected {
            (
                Style::default()
                    .fg(THEME.colors.row_highlight)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(THEME.colors.row_highlight),
                THEME.colors.row_background,
            )
        } else {
            (
                Style::default()
                    .fg(THEME.colors.foreground)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(THEME.colors.text_idle),
                THEME.colors.row_alternate_bg,
            )
        };

        let (status_text, status_style) = if *enabled {
            ("✓", Style::default().fg(THEME.colors.success))
        } else {
            ("✗", Style::default().fg(THEME.colors.error))
        };

        let icon_row_0 = icon_spans(icon_pixels.as_ref(), 0);
        let icon_row_1 = icon_spans(icon_pixels.as_ref(), 1);

        let mut line_0 = icon_row_0;
        line_0.push(Span::raw(" "));
        line_0.push(Span::styled(name.clone(), name_style));
        line_0.push(Span::raw(" "));
        line_0.push(Span::styled(status_text, status_style));

        let mut line_1 = icon_row_1;
        line_1.push(Span::raw(" "));
        line_1.push(Span::styled(description.clone(), description_style));

        let item = Text::from(vec![Line::from(line_0), Line::from(line_1)])
            .style(Style::default().bg(background));
        (item, ITEM_HEIGHT)
    });

    let list = ListView::new(builder, count);
    frame.render_stateful_widget(list, area, &mut state.list_state);
}

fn icon_spans(icon_pixels: Option<&Vec<Vec<IconCell>>>, row: usize) -> Vec<Span<'static>> {
    match icon_pixels.and_then(|rows| rows.get(row)) {
        Some(cols) => cols
            .iter()
            .map(|&(fg_r, fg_g, fg_b, bg_r, bg_g, bg_b)| {
                Span::styled(
                    "▄",
                    Style::default()
                        .fg(Color::Rgb(fg_r, fg_g, fg_b))
                        .bg(Color::Rgb(bg_r, bg_g, bg_b)),
                )
            })
            .collect(),
        None => vec![Span::styled(
            "    ",
            Style::default().fg(THEME.colors.text_idle),
        )],
    }
}
