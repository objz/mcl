use super::*;
use crate::tui::tests::UI_TEST_LOCK;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

// WIZARD_STATE is a process-global static; without serialisation, parallel
// tests would race when each test sets the step and then renders, since
// render re-acquires the WIZARD_STATE mutex internally. this guard mutex
// ensures only one wizard snapshot test runs at a time.
fn reset_wizard_state(step: WizardStep) {
    let mut guard = WIZARD_STATE.lock().expect("WIZARD_STATE lock");
    *guard = WizardState::default();
    guard.step = step;
}

#[test]
fn new_instance_renders_name_step() {
    let _serial = UI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Name is the default step; render touches no network helpers.
    reset_wizard_state(WizardStep::Name);

    let backend = TestBackend::new(60, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| render(f, f.area(), FocusedArea::Popup))
        .unwrap();
    insta::assert_snapshot!(terminal.backend());
}

#[test]
fn new_instance_renders_loader_step() {
    let _serial = UI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Loader step is reached after Name; render just paints the hardcoded
    // loader list, no network.
    reset_wizard_state(WizardStep::Loader);

    let backend = TestBackend::new(60, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| render(f, f.area(), FocusedArea::Popup))
        .unwrap();
    insta::assert_snapshot!(terminal.backend());
}

// Version step: pre-populate versions as LoadState::Loaded so
// ensure_versions_loaded short-circuits and never spawns a network task.
// the three synthetic versions are marked stable=true so they show with
// show_snapshots=false (the default).
#[test]
fn new_instance_renders_version_step() {
    use crate::instance::loader::GameVersion;

    let _serial = UI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    {
        let mut guard = WIZARD_STATE.lock().expect("WIZARD_STATE lock");
        *guard = WizardState::default();
        guard.step = WizardStep::Version;
        guard.versions = LoadState::Loaded(vec![
            GameVersion {
                id: "1.20.1".into(),
                stable: true,
            },
            GameVersion {
                id: "1.19.4".into(),
                stable: true,
            },
            GameVersion {
                id: "1.18.2".into(),
                stable: true,
            },
        ]);
    }

    let backend = TestBackend::new(60, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| render(f, f.area(), FocusedArea::Popup))
        .unwrap();
    insta::assert_snapshot!(terminal.backend());
}

// LoaderVersion step: needs both versions and loader_versions pre-loaded.
// pick a non-Vanilla loader (loader_idx=2 = Forge) so the step doesn't
// skip itself to Confirm.
#[test]
fn new_instance_renders_loader_version_step() {
    use crate::instance::loader::GameVersion;

    let _serial = UI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    {
        let mut guard = WIZARD_STATE.lock().expect("WIZARD_STATE lock");
        *guard = WizardState::default();
        guard.step = WizardStep::LoaderVersion;
        guard.loader_idx = 2; // Forge
        guard.versions = LoadState::Loaded(vec![GameVersion {
            id: "1.20.1".into(),
            stable: true,
        }]);
        guard.loader_versions =
            LoadState::Loaded(vec!["47.2.0".into(), "47.1.0".into(), "47.0.50".into()]);
    }

    let backend = TestBackend::new(60, 14);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| render(f, f.area(), FocusedArea::Popup))
        .unwrap();
    insta::assert_snapshot!(terminal.backend());
}

// Confirm step: paints a summary, no network, no list. requires
// versions + loader_versions Loaded so selected_*() return Some.
#[test]
fn new_instance_renders_confirm_step() {
    use crate::instance::loader::GameVersion;
    use tui_prompts::TextState;

    let _serial = UI_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    {
        let mut guard = WIZARD_STATE.lock().expect("WIZARD_STATE lock");
        *guard = WizardState::default();
        guard.step = WizardStep::Confirm;
        guard.loader_idx = 1; // Fabric
        guard.versions = LoadState::Loaded(vec![GameVersion {
            id: "1.20.1".into(),
            stable: true,
        }]);
        guard.loader_versions = LoadState::Loaded(vec!["0.15.0".into()]);
        // TextState exposes only constructors; rebuilding with the
        // desired initial value is the supported path.
        guard.name_state = TextState::new().with_value("MyPack");
    }

    let backend = TestBackend::new(60, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| render(f, f.area(), FocusedArea::Popup))
        .unwrap();
    insta::assert_snapshot!(terminal.backend());
}
