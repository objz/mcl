// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// Shared selection list used by the new-instance wizard and settings pickers.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::Span,
    widgets::{List, ListItem, ListState, StatefulWidget},
};

use crate::config::theme::THEME;
use crate::instance::models::ModLoader;

pub(crate) const MOD_LOADERS: [ModLoader; 5] = [
    ModLoader::Vanilla,
    ModLoader::Fabric,
    ModLoader::Forge,
    ModLoader::NeoForge,
    ModLoader::Quilt,
];

pub(crate) fn render(items: Vec<ListItem<'_>>, selected: usize, area: Rect, buffer: &mut Buffer) {
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(THEME.as_ref().accent())
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(Span::styled(
            "▶ ",
            Style::default()
                .fg(THEME.as_ref().accent())
                .add_modifier(Modifier::BOLD),
        ));
    let mut state = ListState::default().with_selected(Some(selected));
    StatefulWidget::render(list, area, buffer, &mut state);
}
