use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::instance::{
    ModLoader,
    import::{ImportSummary, PackFormat},
};
use crate::tests::TEST_LOCK;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn summary() -> ImportSummary {
    ImportSummary {
        name: "Test Pack".to_owned(),
        pack_version: "1.0.0".to_owned(),
        game_version: "1.21.1".to_owned(),
        loader: ModLoader::Fabric,
        loader_version: Some("0.16.14".to_owned()),
        mod_count: 2,
        override_count: 1,
        format: PackFormat::Mrpack,
        archive_path: PathBuf::from("test.mrpack"),
    }
}

#[test]
fn empty_input_is_ignored_and_escape_closes_the_popup() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut instances = instances::State {
        show_import_popup: true,
        ..instances::State::default()
    };
    *IMPORT_STATE.lock().unwrap() = ImportWizardState::default();

    handle_key(&key(KeyCode::Enter), &mut instances);
    assert_eq!(IMPORT_STATE.lock().unwrap().step, ImportStep::Input);

    handle_key(&key(KeyCode::Esc), &mut instances);
    assert!(!instances.show_import_popup);
}

#[test]
fn confirm_returns_the_import_summary() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut instances = instances::State {
        show_import_popup: true,
        ..instances::State::default()
    };
    {
        let mut state = IMPORT_STATE.lock().unwrap();
        *state = ImportWizardState {
            step: ImportStep::Confirm,
            summary: Some(summary()),
            ..ImportWizardState::default()
        };
    }
    *IMPORT_RESULT.lock().unwrap() = None;

    handle_key(&key(KeyCode::Enter), &mut instances);
    let result = take_result().expect("import result");
    assert_eq!(result.summary.name, "Test Pack");
    assert!(!instances.show_import_popup);
}
