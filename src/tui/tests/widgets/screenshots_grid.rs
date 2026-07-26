use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use super::*;

fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

fn entry(name: &str) -> ScreenshotEntry {
    ScreenshotEntry {
        name: name.to_owned(),
        path: name.into(),
        width: 1920,
        height: 1080,
    }
}

#[test]
fn grid_navigation_and_search_clamp_the_selection() {
    let mut state = ScreenshotsState {
        entries: vec![entry("one.png"), entry("two.png"), entry("three.png")],
        cols: 2,
        ..Default::default()
    };

    assert!(handle_key(
        &key(KeyCode::Right, KeyModifiers::SHIFT),
        &mut state
    ));
    assert_eq!(state.selected, 1);

    assert!(handle_key(
        &key(KeyCode::Down, KeyModifiers::SHIFT),
        &mut state
    ));
    assert_eq!(state.selected, 1);

    assert!(handle_key(
        &key(KeyCode::Char('/'), KeyModifiers::NONE),
        &mut state
    ));
    assert!(handle_key(
        &key(KeyCode::Char('t'), KeyModifiers::NONE),
        &mut state
    ));
    assert_eq!(state.selected, 0);
    assert_eq!(state.search.query, "t");
}

#[test]
fn unicode_filename_truncation_renders_without_panicking() {
    let mut state = ScreenshotsState {
        entries: vec![entry(&format!("{}é.png", "a".repeat(23)))],
        ..Default::default()
    };
    let backend = TestBackend::new(24, 8);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| render(frame, frame.area(), &mut state, true))
        .unwrap();

    assert!(terminal.backend().to_string().contains("aaaa"));
}
