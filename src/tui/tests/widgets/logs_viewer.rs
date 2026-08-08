use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn list_and_viewer_navigation_loads_and_scrolls_the_selected_log() {
    let temp = tempfile::tempdir().unwrap();
    let first = temp.path().join("first.log");
    let second = temp.path().join("second.log");
    std::fs::write(&first, "first").unwrap();
    std::fs::write(&second, "second\nline").unwrap();

    let mut state = LogsState {
        entries: vec![
            LogFileEntry {
                name: "first.log".to_owned(),
                path: first,
            },
            LogFileEntry {
                name: "second.log".to_owned(),
                path: second,
            },
        ],
        ..Default::default()
    };
    state.list_state.selected = Some(0);

    assert!(handle_key(&key(KeyCode::Down), &mut state));
    assert_eq!(state.list_state.selected, Some(1));
    assert_eq!(state.viewer_lines, ["second", "line"]);

    assert!(handle_key(&key(KeyCode::Enter), &mut state));
    assert!(state.viewer_focused);
    state.viewer_max_scroll = 4;

    assert!(handle_key(&key(KeyCode::Char('G')), &mut state));
    assert_eq!(state.viewer_scroll, 4);
    assert!(handle_key(&key(KeyCode::Char('g')), &mut state));
    assert_eq!(state.viewer_scroll, 0);

    assert!(handle_key(&key(KeyCode::Esc), &mut state));
    assert!(!state.viewer_focused);
}
