use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn offline_account_requires_a_microsoft_account_and_can_be_dismissed() {
    let mut state = AccountState::default();
    state.store.accounts.clear();

    assert!(handle_key(&key(KeyCode::Char('a')), &mut state));
    assert!(matches!(state.add_mode, AddMode::ChooseType));

    assert!(handle_key(&key(KeyCode::Char('o')), &mut state));
    assert!(matches!(state.add_mode, AddMode::OfflineBlocked));

    assert!(handle_key(&key(KeyCode::Esc), &mut state));
    assert!(matches!(state.add_mode, AddMode::None));
}
