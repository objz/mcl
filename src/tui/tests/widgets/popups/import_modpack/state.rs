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
fn empty_input_is_ignored_and_escape_returns_to_discovery() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let mut instances = instances::State {
        show_import_popup: true,
        ..instances::State::default()
    };
    *IMPORT_STATE.lock().unwrap() = ImportWizardState {
        step: ImportStep::Input,
        ..ImportWizardState::default()
    };

    handle_key(&key(KeyCode::Enter), &mut instances);
    assert_eq!(IMPORT_STATE.lock().unwrap().step, ImportStep::Input);

    handle_key(&key(KeyCode::Esc), &mut instances);
    assert_eq!(IMPORT_STATE.lock().unwrap().step, ImportStep::Discover);
    assert!(instances.show_import_popup);

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

#[test]
fn discovered_modpack_becomes_visible_after_its_icon_is_decoded() {
    let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        *DISCOVERY_STATE.lock().unwrap() =
            crate::tui::widgets::content::DiscoveryState::new_modpacks();

        let request = DISCOVERY_STATE.lock().unwrap().begin_modpack_search();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let project = crate::net::modrinth::DiscoveryProject {
            id: "pack-id".to_owned(),
            slug: "test-pack".to_owned(),
            title: "Test Pack".to_owned(),
            description: "A pack".to_owned(),
            downloads: 1,
            icon_url: None,
            icon_bytes: Some(png.into_inner()),
        };
        assert!(request.stream.upsert(
            crate::tui::widgets::content::discovery::provider_project_entry(
                project,
                "modrinth",
                "testpack".to_owned(),
                None,
            )
        ));
        crate::tui::widgets::content::DiscoveryState::push_result(
            &request.pending,
            request.generation,
            request.offset,
            Ok(
                crate::tui::widgets::content::discovery::DiscoveryPageResult {
                    received: 1,
                    total_hits: 1,
                },
            ),
        );

        let picker = ratatui_image::picker::Picker::halfblocks();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                drain(&picker);
                if !DISCOVERY_STATE
                    .lock()
                    .unwrap()
                    .list
                    .filtered_indices()
                    .is_empty()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("modpack icon render completed");
    });
}
