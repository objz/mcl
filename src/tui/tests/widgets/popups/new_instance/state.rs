// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_prompts::{State as PromptState, TextState};

use super::*;
use crate::tests::TEST_LOCK;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn vanilla_wizard_reaches_confirm_and_returns_parameters() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut instances = instances::State {
        show_popup: true,
        ..instances::State::default()
    };

    {
        let mut state = WIZARD_STATE.lock().unwrap();
        *state = WizardState::default();
        state.name_state = TextState::new().with_value("Test Instance");
    }
    *WIZARD_RESULT.lock().unwrap() = None;

    handle_key(&key(KeyCode::Enter), &mut instances);
    assert_eq!(WIZARD_STATE.lock().unwrap().step, WizardStep::Loader);

    {
        let mut state = WIZARD_STATE.lock().unwrap();
        state.step = WizardStep::Version;
        state.versions = LoadState::Loaded(vec![GameVersion {
            id: "1.21.1".to_owned(),
            stable: true,
        }]);
    }
    handle_key(&key(KeyCode::Enter), &mut instances);
    assert_eq!(WIZARD_STATE.lock().unwrap().step, WizardStep::Confirm);

    handle_key(&key(KeyCode::Enter), &mut instances);
    let result = take_result().expect("wizard result");
    assert_eq!(result.name, "Test Instance");
    assert_eq!(result.game_version, "1.21.1");
    assert_eq!(result.loader, ModLoader::Vanilla);
    assert_eq!(result.loader_version, None);
    assert!(!instances.show_popup);
}

#[test]
fn version_filter_clamps_a_stale_selection() {
    let mut state = WizardState {
        versions: LoadState::Loaded(vec![
            GameVersion {
                id: "1.21.1".to_owned(),
                stable: true,
            },
            GameVersion {
                id: "25w01a".to_owned(),
                stable: false,
            },
        ]),
        version_idx: 9,
        ..WizardState::default()
    };

    clamp_version_index(&mut state);
    assert_eq!(state.version_idx, 0);
    assert_eq!(visible_versions(&state).len(), 1);

    state.show_snapshots = true;
    assert_eq!(visible_versions(&state).len(), 2);
}

#[test]
fn ctrl_backspace_deletes_the_word_before_the_name_cursor() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut instances = instances::State::default();

    {
        let mut state = WIZARD_STATE.lock().unwrap();
        *state = WizardState::default();
        state.name_state = TextState::new().with_value("hello brave world");
        *state.name_state.position_mut() = "hello brave".chars().count();
    }

    handle_key(
        &KeyEvent::new(KeyCode::Backspace, KeyModifiers::CONTROL),
        &mut instances,
    );

    let state = WIZARD_STATE.lock().unwrap();
    assert_eq!(state.name_state.value(), "hello  world");
    assert_eq!(state.name_state.position(), "hello ".chars().count());
}
