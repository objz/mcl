// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::time::Instant;
use tracing::Level;

use crate::feedback::errors::ErrorEvent;

fn render(event: ErrorEvent, width: u16, height: u16) -> Terminal<TestBackend> {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let popup = ErrorPopup::new(event);
            f.render_widget(popup, f.area());
        })
        .unwrap();
    terminal
}

fn event(level: Level, message: &str) -> ErrorEvent {
    ErrorEvent {
        id: 1,
        level,
        message: message.to_string(),
        pushed_at: Instant::now(),
    }
}

#[test]
fn warn_level_renders() {
    let term = render(event(Level::WARN, "Disk space low"), 40, 5);
    insta::assert_snapshot!(term.backend());
}

#[test]
fn error_level_renders() {
    let term = render(event(Level::ERROR, "Connection refused"), 40, 5);
    insta::assert_snapshot!(term.backend());
}

// info-level events hit the catch-all `_` arm in the label match; previously
// there was no test covering it.
#[test]
fn info_level_renders() {
    let term = render(event(Level::INFO, "Reloaded config"), 40, 5);
    insta::assert_snapshot!(term.backend());
}

#[test]
fn long_message_wraps() {
    let msg = "The Minecraft launcher could not reach the Mojang version manifest \
                   after three retries. Check your network connection or proxy settings.";
    let term = render(event(Level::ERROR, msg), 40, 10);
    insta::assert_snapshot!(term.backend());
}

#[test]
fn narrow_frame_renders() {
    let term = render(event(Level::WARN, "Short message"), 18, 5);
    insta::assert_snapshot!(term.backend());
}
