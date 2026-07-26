use std::collections::{HashMap, HashSet};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use super::UI_TEST_LOCK;
use crate::instance::{InstanceConfig, InstanceManager, ModLoader};
use crate::tui::{
    app::{App, FocusedArea},
    widgets,
};

pub(super) struct UiHarness {
    pub app: App,
    terminal: Terminal<TestBackend>,
    _temp: tempfile::TempDir,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl UiHarness {
    pub fn new() -> Self {
        let guard = UI_TEST_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        widgets::popups::confirm::clear_pending();
        crate::tui::error_buffer::ERROR_EVENTS
            .lock()
            .expect("error buffer")
            .clear();
        crate::tui::progress::clear();

        let temp = tempfile::tempdir().expect("temporary UI data");
        let instances_dir = temp.path().join("instances");
        let meta_dir = temp.path().join("meta");
        std::fs::create_dir_all(&instances_dir).expect("instances directory");
        std::fs::create_dir_all(&meta_dir).expect("metadata directory");

        let picker = ratatui_image::picker::Picker::halfblocks();
        let font_size = picker.font_size();
        let instance_manager = InstanceManager::new(instances_dir, meta_dir.clone());
        let mut account_state = widgets::account::AccountState::default();
        account_state.store.accounts.clear();
        account_state.list_state.selected = None;

        let app = App {
            exit: false,
            focused: FocusedArea::default(),
            pre_overlay_focused: FocusedArea::default(),
            content_tab: widgets::content::ContentTab::default(),
            content_mode: widgets::content::ContentMode::default(),
            instances_state: widgets::instances::State::default(),
            mods_state: widgets::content::ContentListState::default(),
            mods_discovery_state: widgets::content::DiscoveryState::new(
                crate::net::modrinth::DiscoveryKind::Mod,
            ),
            resource_packs_state: widgets::content::ContentListState::default(),
            resource_packs_discovery_state: widgets::content::DiscoveryState::new(
                crate::net::modrinth::DiscoveryKind::ResourcePack,
            ),
            shaders_state: widgets::content::ContentListState::default(),
            shaders_discovery_state: widgets::content::DiscoveryState::new(
                crate::net::modrinth::DiscoveryKind::Shader,
            ),
            worlds_state: widgets::content::ContentListState::default(),
            screenshots_state: {
                let mut state = widgets::screenshots_grid::ScreenshotsState::default();
                state.font_size = (font_size.width, font_size.height);
                state
            },
            logs_state: widgets::logs_viewer::LogsState::default(),
            account_state,
            settings_state: widgets::settings::SettingsState::new(meta_dir),
            picker,
            instance_manager,
            log_overlay_scroll: 0,
            log_overlay_max_scroll: 0,
            log_overlay_search: widgets::search::SearchState::default(),
            log_overlay_scrollbar: ratatui::widgets::ScrollbarState::default(),
            throbber_state: throbber_widgets_tui::ThrobberState::default(),
            throbber_tick: 0,
            error_effects: HashMap::new(),
            pending_editor: None,
            reconciliation_for: None,
            content_manifest: None,
            provider_conflict: None,
            dismissed_provider_conflicts: HashSet::new(),
        };

        Self {
            app,
            terminal: Terminal::new(TestBackend::new(100, 30)).expect("test terminal"),
            _temp: temp,
            _guard: guard,
        }
    }

    pub fn add_instance(&mut self, name: &str) {
        self.app.instances_state.add_instance(InstanceConfig {
            name: name.to_owned(),
            game_version: "1.21.1".to_owned(),
            loader: ModLoader::Fabric,
            loader_version: Some("0.16.14".to_owned()),
            created: chrono::Utc::now(),
            last_played: None,
            java_path: None,
            memory_max: None,
            memory_min: None,
            jvm_args: Vec::new(),
            resolution: None,
            config_sync_profile: None,
        });
    }

    pub fn key(&mut self, code: KeyCode) {
        self.key_with(code, KeyModifiers::NONE);
    }

    pub fn key_with(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.app
            .handle_key_event(KeyEvent::new(code, modifiers))
            .expect("handle key");
    }

    pub fn draw(&mut self) {
        let app = &mut self.app;
        self.terminal
            .draw(|frame| app.render_frame(frame))
            .expect("draw UI");
    }

    pub fn screen(&self) -> String {
        self.terminal.backend().to_string()
    }
}
