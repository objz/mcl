use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn offline_account_requires_a_microsoft_account_and_can_be_dismissed() {
    let temp = tempfile::tempdir().unwrap();
    let mut state = AccountState {
        store: AccountStore::empty_for_test(temp.path().join("accounts.json")),
        list_state: Default::default(),
        add_mode: AddMode::None,
    };

    assert!(handle_key(&key(KeyCode::Char('a')), &mut state));
    assert!(matches!(state.add_mode, AddMode::ChooseType));

    assert!(handle_key(&key(KeyCode::Char('o')), &mut state));
    assert!(matches!(state.add_mode, AddMode::OfflineBlocked));

    assert!(handle_key(&key(KeyCode::Esc), &mut state));
    assert!(matches!(state.add_mode, AddMode::None));
}
