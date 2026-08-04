// keybindings and input dispatch.
// the general pattern: check which area is focused, give it first crack at the
// keypress, and fall through to global bindings if nobody claimed it.
// vim-style navigation (j/k/g/G) where it makes sense.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use super::app::{App, FocusedArea};
use super::widgets::{
    self, WidgetKey, popups::confirm as confirm_popup, popups::import_modpack, popups::new_instance,
};
use crate::feedback::errors as error_buffer;

impl App {
    pub(super) fn handle_mouse_event(&mut self, event: MouseEvent) {
        let scroll_key = match event.kind {
            MouseEventKind::ScrollUp => Some(KeyCode::Up),
            MouseEventKind::ScrollDown => Some(KeyCode::Down),
            _ => None,
        };
        if let Some(code) = scroll_key {
            if let Err(error) = self.handle_key_event(KeyEvent::new(code, event.modifiers)) {
                tracing::error!("Mouse scroll handling failed: {error}");
            }
            return;
        }
        if event.kind != MouseEventKind::Down(MouseButton::Left) {
            return;
        }
        if self.focused == FocusedArea::ImportPopup
            && import_modpack::handle_discovery_click(event.column, event.row)
        {
            return;
        }
        if self.focused != FocusedArea::Content
            || self.content_mode != widgets::content::ContentMode::Discover
        {
            return;
        }
        let page_clicked = self.active_discovery_state_mut().is_some_and(|state| {
            !state.search.active
                && !state.project_page_open()
                && state.version_popup.is_none()
                && state.list.click_page(event.column, event.row)
        });
        if page_clicked {
            self.spawn_active_discovery_page();
            return;
        }
        let link = self
            .active_discovery_state_mut()
            .and_then(|state| state.project_link_at(event.column, event.row));
        if let Some(link) = link
            && let Err(error) = open::that_detached(link)
        {
            tracing::warn!("Failed to open project link {link}: {error}");
        }
    }

    pub(super) fn handle_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        if let Some(conflict) = self.provider_conflict.as_mut() {
            match key_event.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    conflict.selected =
                        (conflict.selected + 1).min(conflict.candidates.len().saturating_sub(1));
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    conflict.selected = conflict.selected.saturating_sub(1);
                }
                KeyCode::Enter => {
                    let relative_path = conflict.relative_path.clone();
                    let project = conflict.candidates.get(conflict.selected).cloned();
                    if let Some(project) = project
                        && let Some((instance_name, _)) = &self.content_manifest
                    {
                        let manifest_path = crate::storage::InstancePaths::new(
                            self.instance_manager.instances_dir.join(instance_name),
                        )
                        .content_manifest();
                        let updated =
                            crate::instance::ContentManifest::update(&manifest_path, |manifest| {
                                if let Some(record) = manifest
                                    .files
                                    .iter_mut()
                                    .find(|record| record.relative_path == relative_path)
                                {
                                    record.resolution =
                                        crate::instance::Resolution::Resolved { project };
                                }
                                Ok(manifest.clone())
                            })?;
                        self.content_manifest = Some((instance_name.clone(), updated));
                        self.provider_conflict = None;
                        self.reconciliation_for = None;
                    }
                }
                KeyCode::Esc => {
                    self.dismissed_provider_conflicts
                        .insert(conflict.relative_path.clone());
                    self.provider_conflict = None;
                }
                _ => {}
            }
            return Ok(());
        }

        // log overlay eats all input when open, including its own search sub-mode
        if self.focused == FocusedArea::OverviewExpanded {
            if self.log_overlay_search.active {
                match key_event.code {
                    KeyCode::Enter => {
                        self.log_overlay_search.confirm();
                    }
                    KeyCode::Esc => {
                        self.log_overlay_search.deactivate();
                    }
                    KeyCode::Backspace => {
                        self.log_overlay_search.backspace(key_event.modifiers);
                    }
                    KeyCode::Char(c) => {
                        self.log_overlay_search.push(c);
                    }
                    _ => {}
                }
                return Ok(());
            }
            match key_event.code {
                KeyCode::Char('O') | KeyCode::Esc => {
                    self.focused = self.pre_overlay_focused;
                    self.log_overlay_search.deactivate();
                    return Ok(());
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if self.log_overlay_scroll < self.log_overlay_max_scroll {
                        self.log_overlay_scroll += 1;
                    }
                    return Ok(());
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.log_overlay_scroll = self.log_overlay_scroll.saturating_sub(1);
                    return Ok(());
                }
                KeyCode::Char('G') => {
                    self.log_overlay_scroll = self.log_overlay_max_scroll;
                    return Ok(());
                }
                KeyCode::Char('g') => {
                    self.log_overlay_scroll = 0;
                    return Ok(());
                }
                KeyCode::Char('/') => {
                    self.log_overlay_search.activate();
                    return Ok(());
                }
                _ => {
                    return Ok(());
                }
            }
        }

        if self.focused == FocusedArea::ConfirmDelete {
            match key_event.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let focus_after = match confirm_popup::pending_target() {
                        Some(confirm_popup::ConfirmTarget::Instance { name }) => {
                            match self.instance_manager.delete(&name) {
                                Ok(_) => {
                                    self.instances_state.remove_instance(&name);
                                }
                                Err(e) => {
                                    tracing::error!("Failed to delete instance '{}': {}", name, e);
                                }
                            }
                            FocusedArea::Instances
                        }
                        Some(confirm_popup::ConfirmTarget::Account { index, .. }) => {
                            let count = self.account_state.store.accounts.len();
                            self.account_state.store.remove(index);
                            if count > 1 {
                                self.account_state.list_state.selected = Some(index.min(
                                    self.account_state.store.accounts.len().saturating_sub(1),
                                ));
                            } else {
                                self.account_state.list_state.selected = None;
                            }
                            FocusedArea::Account
                        }
                        Some(confirm_popup::ConfirmTarget::ConfigProfile { profile }) => {
                            if let Err(e) = self.delete_config_profile(&profile) {
                                error_buffer::push_error(error_buffer::ErrorEvent {
                                    id: 0,
                                    level: tracing::Level::ERROR,
                                    message: e.to_string(),
                                    pushed_at: std::time::Instant::now(),
                                });
                            }
                            FocusedArea::Settings
                        }
                        Some(confirm_popup::ConfirmTarget::Content { name, path, .. }) => {
                            let orphaned = self.orphan_dependencies_after_removing(&path);
                            match delete_content_path(&path) {
                                Ok(()) => {
                                    self.remove_content_path_from_states(&path);
                                    self.remove_content_path_from_manifest(&path);
                                    if !orphaned.is_empty() {
                                        confirm_popup::set_pending_orphan_dependencies(orphaned);
                                        return Ok(());
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Failed to delete content '{}': {}", name, e);
                                }
                            }
                            FocusedArea::Content
                        }
                        Some(confirm_popup::ConfirmTarget::OrphanDependencies { paths }) => {
                            for path in paths {
                                match delete_content_path(&path) {
                                    Ok(()) => {
                                        self.remove_content_path_from_states(&path);
                                        self.remove_content_path_from_manifest(&path);
                                    }
                                    Err(error) => tracing::error!(
                                        "Failed to remove unused dependency '{}': {}",
                                        path.display(),
                                        error
                                    ),
                                }
                            }
                            FocusedArea::Content
                        }
                        None => FocusedArea::Instances,
                    };
                    confirm_popup::clear_pending();
                    self.focused = focus_after;
                    return Ok(());
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    let focus_after = match confirm_popup::pending_target() {
                        Some(confirm_popup::ConfirmTarget::Content { .. }) => FocusedArea::Content,
                        Some(confirm_popup::ConfirmTarget::OrphanDependencies { .. }) => {
                            FocusedArea::Content
                        }
                        Some(confirm_popup::ConfirmTarget::Account { .. }) => FocusedArea::Account,
                        Some(confirm_popup::ConfirmTarget::ConfigProfile { .. }) => {
                            FocusedArea::Settings
                        }
                        _ => FocusedArea::Instances,
                    };
                    confirm_popup::clear_pending();
                    self.focused = focus_after;
                    return Ok(());
                }
                _ => {
                    return Ok(());
                }
            }
        }

        // content area delegates to whichever tab is active.
        // worlds use the same list navigation without the toggle
        if self.focused == FocusedArea::Content
            && self.content_mode == widgets::content::ContentMode::Discover
        {
            let (search_active, popup_open, project_page_open) = self
                .active_discovery_state_mut()
                .map(|state| {
                    (
                        state.search.active,
                        state.version_popup.is_some(),
                        state.project_page_open(),
                    )
                })
                .unwrap_or_default();
            if !search_active
                && !popup_open
                && !project_page_open
                && key_event.code == KeyCode::Enter
            {
                self.spawn_active_discovery_project_page();
                return Ok(());
            }
            if !search_active && !popup_open && key_event.code == KeyCode::Char('v') {
                self.spawn_active_discovery_versions();
                return Ok(());
            }
            if !search_active
                && !popup_open
                && !project_page_open
                && key_event.code == KeyCode::Char('d')
            {
                if let Some(pending) = self
                    .active_discovery_state_mut()
                    .and_then(|state| state.pending_installed_delete())
                {
                    self.confirm_content_delete(pending);
                }
                return Ok(());
            }
            if popup_open && key_event.code == KeyCode::Enter {
                if self
                    .active_discovery_state_mut()
                    .is_some_and(|state| state.select_minecraft_version())
                {
                    return Ok(());
                }
                let selecting_world = self
                    .active_discovery_state_mut()
                    .and_then(|state| state.version_popup.as_ref())
                    .is_some_and(|popup| popup.selecting_world);
                if selecting_world {
                    let minecraft_dir = self.instances_state.selected_instance().map(|instance| {
                        crate::storage::InstancePaths::new(
                            self.instance_manager.instances_dir.join(&instance.name),
                        )
                        .minecraft()
                    });
                    let manifest = self
                        .content_manifest
                        .as_ref()
                        .map(|(_, manifest)| manifest.clone());
                    if let Some(minecraft_dir) = minecraft_dir
                        && self.active_discovery_state_mut().is_some_and(|state| {
                            state.select_world(manifest.as_ref(), &minecraft_dir)
                        })
                    {
                        self.spawn_active_discovery_dependencies();
                    }
                    return Ok(());
                }
                let confirming = self
                    .active_discovery_state_mut()
                    .and_then(|state| state.version_popup.as_ref())
                    .is_some_and(|popup| popup.confirming);
                if confirming {
                    self.spawn_active_discovery_install();
                } else {
                    let kind = self.active_discovery_state_mut().map(|state| state.kind);
                    if kind == Some(crate::instance::ContentKind::DataPack) {
                        let worlds = self.worlds_state.entries.clone();
                        if let Some(state) = self.active_discovery_state_mut() {
                            state.begin_world_selection(worlds);
                        }
                    } else if kind == Some(crate::instance::ContentKind::Mod) {
                        self.spawn_active_discovery_dependencies();
                    } else if let Some(state) = self.active_discovery_state_mut() {
                        state.begin_confirmation();
                    }
                }
                return Ok(());
            }
            if popup_open && key_event.code == KeyCode::Tab {
                self.spawn_active_discovery_version_source();
                return Ok(());
            }
            let handled = self
                .active_discovery_state_mut()
                .is_some_and(|state| widgets::content::discovery::handle_key(&key_event, state));
            if handled {
                if matches!(
                    key_event.code,
                    KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Down | KeyCode::Up
                ) || widgets::content::discovery::page_key_direction(&key_event).is_some()
                {
                    self.spawn_active_discovery_page();
                }
                return Ok(());
            }
        } else if self.focused == FocusedArea::Content {
            if self.content_tab == widgets::content::ContentTab::Logs {
                if key_event.code == KeyCode::Char('d')
                    && !self.logs_state.search.active
                    && !self.logs_state.viewer_search.active
                {
                    if let Some(pending) = self.logs_state.pending_delete() {
                        self.confirm_content_delete(pending);
                    }
                    return Ok(());
                }
                if widgets::logs_viewer::handle_key(&key_event, &mut self.logs_state) {
                    return Ok(());
                }
            } else if self.content_tab == widgets::content::ContentTab::Screenshots {
                if key_event.code == KeyCode::Char('d') && !self.screenshots_state.search.active {
                    if let Some(pending) = self.screenshots_state.pending_delete() {
                        self.confirm_content_delete(pending);
                    }
                    return Ok(());
                }
                if widgets::screenshots_grid::handle_key(&key_event, &mut self.screenshots_state) {
                    return Ok(());
                }
            } else if self.content_tab == widgets::content::ContentTab::Worlds {
                if self.open_world_datapacks.is_some() {
                    if matches!(key_event.code, KeyCode::Esc | KeyCode::Char('h'))
                        && !self.world_datapacks_state.search.active
                    {
                        self.open_world_datapacks = None;
                        return Ok(());
                    }
                    if key_event.code == KeyCode::Char('d')
                        && !self.world_datapacks_state.search.active
                    {
                        if let Some(pending) = self.world_datapacks_state.pending_delete() {
                            self.confirm_content_delete(pending);
                        }
                        return Ok(());
                    }
                    if widgets::content::list::handle_key_no_toggle(
                        &key_event,
                        &mut self.world_datapacks_state,
                    ) {
                        return Ok(());
                    }
                    return Ok(());
                }
                if key_event.code == KeyCode::Enter && !self.worlds_state.search.active {
                    self.open_world_datapacks = self
                        .worlds_state
                        .selected_entry()
                        .map(|entry| (entry.name.clone(), entry.path.clone()));
                    return Ok(());
                }
                if key_event.code == KeyCode::Char('q') && !self.worlds_state.search.active {
                    if self.selected_instance_supports_quick_play() {
                        let instance = self.instances_state.selected_instance().cloned();
                        let world = self
                            .worlds_state
                            .selected_entry()
                            .and_then(|entry| entry.path.file_name())
                            .and_then(|name| name.to_str())
                            .map(str::to_owned);
                        if let (Some(instance), Some(world)) = (instance, world) {
                            self.spawn_launch(instance, Some(world));
                        }
                    }
                    return Ok(());
                }
                if key_event.code == KeyCode::Char('d') && !self.worlds_state.search.active {
                    if let Some(pending) = self.worlds_state.pending_delete() {
                        self.confirm_content_delete(pending);
                    }
                    return Ok(());
                }
                if widgets::content::list::handle_key_no_toggle(&key_event, &mut self.worlds_state)
                {
                    return Ok(());
                }
            } else {
                let state = match self.content_tab {
                    widgets::content::ContentTab::Mods => Some(&mut self.mods_state),
                    widgets::content::ContentTab::ResourcePacks => {
                        Some(&mut self.resource_packs_state)
                    }
                    widgets::content::ContentTab::Shaders => Some(&mut self.shaders_state),
                    _ => None,
                };
                if let Some(state) = state {
                    if key_event.code == KeyCode::Char('d') && !state.search.active {
                        if let Some(pending) = state.pending_delete() {
                            self.confirm_content_delete(pending);
                        }
                        return Ok(());
                    }
                    if widgets::content::list::handle_key(&key_event, state) {
                        return Ok(());
                    }
                }
            }
        }

        if self.focused == FocusedArea::Account
            && let KeyCode::Char('d') = key_event.code
            && let Some(index) = self.account_state.list_state.selected
            && let Some(account) = self.account_state.store.accounts.get(index)
        {
            confirm_popup::set_pending(confirm_popup::ConfirmTarget::Account {
                username: account.username.clone(),
                index,
            });
            self.focused = FocusedArea::ConfirmDelete;
            return Ok(());
        }

        if self.focused == FocusedArea::Account
            && widgets::account::handle_key(&key_event, &mut self.account_state)
        {
            return Ok(());
        }

        if self.focused == FocusedArea::Settings {
            match widgets::settings::handle_key(
                &key_event,
                &mut self.settings_state,
                self.instances_state.selected_instance(),
                &self.instance_manager.instances_dir,
            ) {
                widgets::settings::SettingsAction::EditInstance(path)
                | widgets::settings::SettingsAction::EditGlobal(path) => {
                    self.pending_editor = Some(path);
                    return Ok(());
                }
                widgets::settings::SettingsAction::ToggleDesktop => {
                    if let Some(inst) = self.instances_state.selected_instance() {
                        let name = inst.name.clone();
                        match crate::instance::desktop::toggle(inst) {
                            Ok(true) => {
                                error_buffer::push_error(error_buffer::ErrorEvent {
                                    id: 0,
                                    level: tracing::Level::INFO,
                                    message: format!("Desktop shortcut created for '{name}'"),
                                    pushed_at: std::time::Instant::now(),
                                });
                            }
                            Ok(false) => {
                                error_buffer::push_error(error_buffer::ErrorEvent {
                                    id: 0,
                                    level: tracing::Level::INFO,
                                    message: format!("Desktop shortcut removed for '{name}'"),
                                    pushed_at: std::time::Instant::now(),
                                });
                            }
                            Err(e) => {
                                tracing::error!("Failed to toggle desktop shortcut: {}", e);
                            }
                        }
                    }
                    return Ok(());
                }
                widgets::settings::SettingsAction::SelectProfile(profile) => {
                    if let Some(inst) = self.instances_state.selected_instance().cloned() {
                        let instance_dir = self.instance_manager.instances_dir.join(&inst.name);
                        match crate::instance::config_sync::switch_profile(
                            &inst.name,
                            inst.config_sync_profile.as_deref(),
                            profile.as_deref(),
                            &self.instance_manager.meta_dir,
                            &instance_dir,
                        ) {
                            Ok(selected) => {
                                let mut updated = inst.clone();
                                updated.config_sync_profile = selected;
                                if let Err(e) = self.instance_manager.save(&updated) {
                                    tracing::error!("Failed to save config profile: {}", e);
                                } else {
                                    self.instances_state.replace_instance(&inst.name, updated);
                                }
                            }
                            Err(e) => {
                                error_buffer::push_error(error_buffer::ErrorEvent {
                                    id: 0,
                                    level: tracing::Level::ERROR,
                                    message: e.to_string(),
                                    pushed_at: std::time::Instant::now(),
                                });
                            }
                        }
                    }
                    return Ok(());
                }
                widgets::settings::SettingsAction::ConfirmDeleteProfile(profile) => {
                    confirm_popup::set_pending(confirm_popup::ConfirmTarget::ConfigProfile {
                        profile,
                    });
                    self.focused = FocusedArea::ConfirmDelete;
                    return Ok(());
                }
                widgets::settings::SettingsAction::Error(message) => {
                    error_buffer::push_error(error_buffer::ErrorEvent {
                        id: 0,
                        level: tracing::Level::ERROR,
                        message,
                        pushed_at: std::time::Instant::now(),
                    });
                    return Ok(());
                }
                widgets::settings::SettingsAction::None => {}
            }
        }

        match self.focused {
            FocusedArea::Popup => {
                new_instance::handle_key(&key_event, &mut self.instances_state);
            }
            FocusedArea::ImportPopup => {
                import_modpack::handle_key(&key_event, &mut self.instances_state);
            }
            _ => {
                if self.focused == FocusedArea::Instances && self.instances_state.renaming.is_some()
                {
                    match key_event.code {
                        KeyCode::Enter => {
                            let new_name = self.instances_state.renaming.take().unwrap_or_default();
                            if let Some(inst) = self.instances_state.selected_instance() {
                                let old_name = inst.name.clone();
                                if let Ok(()) = self.instance_manager.rename(&old_name, &new_name)
                                    && let Some(inst) = self
                                        .instances_state
                                        .instances
                                        .iter_mut()
                                        .find(|i| i.name == old_name)
                                {
                                    inst.name = new_name.trim().to_owned();
                                }
                            }
                        }
                        KeyCode::Esc => {
                            self.instances_state.renaming = None;
                        }
                        KeyCode::Backspace => {
                            if let Some(ref mut name) = self.instances_state.renaming {
                                widgets::search::backspace(name, key_event.modifiers);
                            }
                        }
                        KeyCode::Char(c) => {
                            if let Some(ref mut name) = self.instances_state.renaming {
                                name.push(c);
                            }
                        }
                        _ => {}
                    }
                    return Ok(());
                }

                if self.focused == FocusedArea::Instances && self.instances_state.search.active {
                    self.instances_state.handle_key(&key_event);
                    return Ok(());
                }

                // global keybindings (uppercase = area switch, lowercase = action)
                match key_event.code {
                    KeyCode::Char('q') => self.exit = true,
                    KeyCode::Char('I') => self.focused = FocusedArea::Instances,
                    KeyCode::Char('C') => self.focused = FocusedArea::Content,
                    KeyCode::Char('A') => self.focused = FocusedArea::Account,
                    KeyCode::Char('S') => self.focused = FocusedArea::Settings,
                    KeyCode::Char('O') => {
                        self.pre_overlay_focused = self.focused;
                        self.focused = FocusedArea::OverviewExpanded;
                    }
                    KeyCode::Tab if self.focused == FocusedArea::Content => {
                        self.content_mode = self.content_mode.toggle();
                        self.content_tab = match (self.content_mode, self.content_tab) {
                            (
                                widgets::content::ContentMode::Installed,
                                widgets::content::ContentTab::DataPacks,
                            ) => widgets::content::ContentTab::Worlds,
                            (
                                widgets::content::ContentMode::Discover,
                                widgets::content::ContentTab::Worlds,
                            ) => widgets::content::ContentTab::DataPacks,
                            (widgets::content::ContentMode::Discover, tab)
                                if !matches!(
                                    tab,
                                    widgets::content::ContentTab::Mods
                                        | widgets::content::ContentTab::ResourcePacks
                                        | widgets::content::ContentTab::Shaders
                                        | widgets::content::ContentTab::DataPacks
                                ) =>
                            {
                                widgets::content::ContentTab::Mods
                            }
                            (_, tab) => tab,
                        };
                        if self.content_mode == widgets::content::ContentMode::Discover {
                            self.open_world_datapacks = None;
                        }
                        self.ensure_active_discovery_loaded();
                    }
                    KeyCode::Char('l') | KeyCode::Right if self.focused == FocusedArea::Content => {
                        self.content_tab = self.content_tab.next_for_mode(self.content_mode);
                        self.ensure_active_discovery_loaded();
                    }
                    KeyCode::Char('h') | KeyCode::Left if self.focused == FocusedArea::Content => {
                        self.content_tab = self.content_tab.previous_for_mode(self.content_mode);
                        self.ensure_active_discovery_loaded();
                    }
                    KeyCode::Char('d')
                        if self.focused == FocusedArea::Instances
                            && !self.instances_state.search.active =>
                    {
                        if let Some(instance) = self.instances_state.selected_instance() {
                            let name = instance.name.clone();
                            confirm_popup::set_pending_instance_delete(&name);
                            self.focused = FocusedArea::ConfirmDelete;
                        }
                    }
                    // shift+enter = open .minecraft folder in file manager
                    KeyCode::Enter
                        if self.focused == FocusedArea::Instances
                            && !self.instances_state.search.active
                            && key_event.modifiers.contains(KeyModifiers::SHIFT) =>
                    {
                        if let Some(instance) = self.instances_state.selected_instance() {
                            let dir = self
                                .instance_manager
                                .instances_dir
                                .join(&instance.name)
                                .join(crate::storage::MINECRAFT_DIR_NAME);
                            if let Err(e) = open::that_detached(&dir) {
                                tracing::error!("Failed to open instance directory: {}", e);
                            }
                        }
                    }
                    // plain enter = focus the content area for the selected instance
                    KeyCode::Enter
                        if self.focused == FocusedArea::Instances
                            && !self.instances_state.search.active =>
                    {
                        self.focused = FocusedArea::Content;
                    }
                    // only allow launching if instance isn't already running.
                    // crashed instances can be relaunched (clears old state first)
                    KeyCode::Char('l')
                        if self.focused == FocusedArea::Instances
                            && !self.instances_state.search.active =>
                    {
                        if let Some(instance) = self.instances_state.selected_instance().cloned() {
                            self.spawn_launch(instance, None);
                        }
                    }
                    KeyCode::Char('r')
                        if self.focused == FocusedArea::Instances
                            && !self.instances_state.search.active =>
                    {
                        if let Some(inst) = self.instances_state.selected_instance() {
                            self.instances_state.renaming = Some(inst.name.clone());
                        }
                    }
                    // esc = kill running instance. brutal but effective
                    KeyCode::Esc
                        if self.focused == FocusedArea::Instances
                            && !self.instances_state.search.active =>
                    {
                        if let Some(instance) = self.instances_state.selected_instance() {
                            crate::instance::runtime::send_kill(&instance.name);
                        }
                    }
                    _ => {}
                }

                if self.focused == FocusedArea::Instances {
                    self.instances_state.handle_key(&key_event)
                }
            }
        }

        if self.instances_state.wants_popup() {
            self.focused = FocusedArea::Popup;
        } else if self.focused == FocusedArea::Popup {
            self.focused = FocusedArea::Instances;
        }

        if self.instances_state.wants_import_popup() {
            if self.focused != FocusedArea::ImportPopup {
                import_modpack::open();
            }
            self.focused = FocusedArea::ImportPopup;
        } else if self.focused == FocusedArea::ImportPopup {
            self.focused = FocusedArea::Instances;
        }

        Ok(())
    }

    pub(super) fn ensure_active_discovery_loaded(&mut self) {
        if self.content_mode != widgets::content::ContentMode::Discover {
            return;
        }
        let Some(instance) = self.instances_state.selected_instance().cloned() else {
            return;
        };
        let Some(state) = self.active_discovery_state_mut() else {
            return;
        };
        if state.unavailable_message(&instance).is_some() {
            state.set_unavailable(&instance);
            return;
        }
        let needs_search = state.needs_search(&instance);
        let search_due = state.search_due();
        if needs_search || search_due {
            self.spawn_active_discovery_search();
        } else {
            self.spawn_active_discovery_page();
        }
    }

    fn active_discovery_state_mut(
        &mut self,
    ) -> Option<&mut widgets::content::discovery::DiscoveryState> {
        match self.content_tab {
            widgets::content::ContentTab::Mods => Some(&mut self.mods_discovery_state),
            widgets::content::ContentTab::ResourcePacks => {
                Some(&mut self.resource_packs_discovery_state)
            }
            widgets::content::ContentTab::Shaders => Some(&mut self.shaders_discovery_state),
            widgets::content::ContentTab::DataPacks => Some(&mut self.datapacks_discovery_state),
            _ => None,
        }
    }

    fn spawn_active_discovery_versions(&mut self) {
        let Some(instance) = self.instances_state.selected_instance().cloned() else {
            return;
        };
        let Some(state) = self.active_discovery_state_mut() else {
            return;
        };
        let kind = state.kind;
        let Some(request) = state.begin_versions() else {
            return;
        };
        self.spawn_discovery_versions_request(instance, kind, request);
    }

    fn spawn_active_discovery_version_source(&mut self) {
        let Some(instance) = self.instances_state.selected_instance().cloned() else {
            return;
        };
        let Some(state) = self.active_discovery_state_mut() else {
            return;
        };
        let kind = state.kind;
        let Some(request) = state.switch_version_source() else {
            return;
        };
        self.spawn_discovery_versions_request(instance, kind, request);
    }

    fn spawn_active_discovery_dependencies(&mut self) {
        let Some(instance) = self.instances_state.selected_instance().cloned() else {
            return;
        };
        let Some(request) = self
            .active_discovery_state_mut()
            .and_then(|state| state.begin_dependency_resolution())
        else {
            return;
        };
        let paths = crate::storage::InstancePaths::new(
            self.instance_manager.instances_dir.join(&instance.name),
        );
        tokio::spawn(async move {
            let result = async {
                let manifest = crate::instance::ContentManifest::load(&paths.content_manifest())
                    .map_err(|error| crate::net::NetError::Parse(error.to_string()))?;
                let registry = crate::instance::content::provider::ProviderRegistry::configured(
                    crate::net::HttpClient::new(),
                );
                crate::instance::content::dependencies::resolve(
                    &registry,
                    &manifest,
                    &paths.minecraft(),
                    &instance,
                    request.root,
                )
                .await
            }
            .await
            .map_err(|error| error.to_string());
            widgets::content::DiscoveryState::push_action_result(
                &request.pending,
                widgets::content::discovery::DiscoveryActionResult::Dependencies {
                    request_id: request.request_id,
                    project_id: request.project_id,
                    result,
                },
            );
        });
    }

    fn spawn_discovery_versions_request(
        &self,
        instance: crate::instance::InstanceConfig,
        kind: crate::instance::ContentKind,
        request: widgets::content::discovery::VersionsRequest,
    ) {
        let version_cache = crate::storage::MetadataPaths::new(&self.instance_manager.meta_dir)
            .provider_versions(&request.provider)
            .join(&request.project_id)
            .join(format!(
                "{}-{}.json",
                instance.game_version,
                instance.loader.to_string().to_lowercase()
            ));
        tokio::spawn(async move {
            let registry = crate::instance::content::provider::ProviderRegistry::configured(
                crate::net::HttpClient::new(),
            );
            let result = match registry.get(&request.provider) {
                Some(provider) => match provider
                    .compatible_versions(
                        &request.project_id,
                        kind,
                        &instance.game_version,
                        instance.loader,
                    )
                    .await
                {
                    Ok(versions) => {
                        if let Ok(bytes) = serde_json::to_vec_pretty(&versions) {
                            let _ = crate::storage::write_atomic(&version_cache, &bytes);
                        }
                        Ok(versions)
                    }
                    Err(error) => match std::fs::read(&version_cache)
                        .ok()
                        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                    {
                        Some(versions) => Ok(versions),
                        None => Err(error.to_string()),
                    },
                },
                None => Err(format!(
                    "{} content provider is unavailable",
                    request.provider
                )),
            };
            widgets::content::DiscoveryState::push_action_result(
                &request.pending,
                widgets::content::discovery::DiscoveryActionResult::Versions {
                    request_id: request.request_id,
                    project_id: request.project_id,
                    result,
                },
            );
        });
    }

    fn spawn_active_discovery_project_page(&mut self) {
        let Some(request) = self
            .active_discovery_state_mut()
            .and_then(widgets::content::DiscoveryState::begin_project_page)
        else {
            return;
        };
        widgets::content::discovery::spawn_project_page(request);
    }

    fn spawn_active_discovery_install(&mut self) {
        let Some(instance) = self.instances_state.selected_instance().cloned() else {
            return;
        };
        let Some(state) = self.active_discovery_state_mut() else {
            return;
        };
        let kind = state.kind;
        let Some(request) = state.begin_install() else {
            return;
        };
        let instances_dir = self.instance_manager.instances_dir.clone();
        let instance_paths = crate::storage::InstancePaths::new(instances_dir.join(&instance.name));
        let manifest_path = instance_paths.content_manifest();
        let minecraft_dir = instance_paths.minecraft();
        let destination = match (&request.target_world, kind) {
            (Some((_, world)), crate::instance::ContentKind::DataPack) => world.join("datapacks"),
            _ => minecraft_dir.join(kind.directory()),
        };
        tokio::spawn(async move {
            let action = if request.installed_path.is_some() {
                format!("Changing {} version", request.project_title)
            } else {
                format!("Installing {}", request.project_title)
            };
            let progress = crate::feedback::progress::ProgressTask::start(action);
            progress.set_sub_action(&request.version.version_number);
            let client = crate::net::HttpClient::new();
            let result = async {
                if kind == crate::instance::ContentKind::DataPack
                    && (request.target_world.is_none()
                        || !destination.starts_with(minecraft_dir.join("saves")))
                {
                    return Err(crate::net::NetError::Parse(
                        "Invalid datapack target world".to_owned(),
                    ));
                }
                tokio::fs::create_dir_all(&destination)
                    .await
                    .map_err(crate::net::NetError::from)?;
                let registry =
                    crate::instance::content::provider::ProviderRegistry::configured(client);
                if let Some(plan) = &request.dependency_plan {
                    let installed = crate::instance::content::dependencies::install(
                        &registry,
                        &manifest_path,
                        &minecraft_dir,
                        plan,
                    )
                    .await?;
                    return Ok::<_, crate::net::NetError>(
                        widgets::content::discovery::InstallCompletion {
                            path: installed.root_path,
                            replaced: installed.replaced,
                            skipped: installed.skipped,
                            orphaned_dependencies: installed.orphaned_dependencies,
                        },
                    );
                }
                let provider = registry.get(&request.provider).ok_or_else(|| {
                    crate::net::NetError::Parse(format!(
                        "{} content provider is unavailable",
                        request.provider
                    ))
                })?;
                let outcome = provider
                    .download_version(
                        &request.version,
                        &destination,
                        request.installed_path.as_deref(),
                    )
                    .await?;
                let (path, skipped) = match outcome {
                    crate::net::modrinth::DownloadOutcome::Downloaded(path) => (path, false),
                    crate::net::modrinth::DownloadOutcome::SkippedExisting(path) => (path, true),
                };
                let replaced = request.installed_path.is_some() && !skipped;
                let relative_path = path
                    .strip_prefix(&minecraft_dir)
                    .map_err(|error| crate::net::NetError::Parse(error.to_string()))?
                    .to_path_buf();
                let fingerprint = crate::instance::content::manifest::fingerprint(&path)?;
                let record = crate::instance::ContentFileRecord {
                    relative_path,
                    kind,
                    enabled: !path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.ends_with(".disabled")),
                    fingerprint,
                    resolution: crate::instance::Resolution::Resolved {
                        project: crate::instance::ProviderProject {
                            provider: request.provider.clone(),
                            project_id: request.project_id.clone(),
                            version_id: request.version.id.clone(),
                        },
                    },
                    provider_aliases: Vec::new(),
                    required_dependencies: Vec::new(),
                    automatic_dependency: false,
                    cleanup_eligible: false,
                };
                crate::instance::ContentManifest::update(&manifest_path, |manifest| {
                    if let Some(old_path) = request.installed_path.as_ref()
                        && let Ok(relative) = old_path.strip_prefix(&minecraft_dir)
                        && old_path != &path
                    {
                        manifest.remove(relative);
                    }
                    manifest.upsert(record);
                    Ok(())
                })
                .map_err(|error| crate::net::NetError::Parse(error.to_string()))?;
                if !skipped
                    && let Some(old_path) = request
                        .installed_path
                        .as_ref()
                        .filter(|old_path| old_path.as_path() != path.as_path())
                {
                    delete_content_path(old_path).map_err(crate::net::NetError::from)?;
                }
                Ok::<_, crate::net::NetError>(widgets::content::discovery::InstallCompletion {
                    path,
                    replaced,
                    skipped,
                    orphaned_dependencies: Vec::new(),
                })
            }
            .await
            .map_err(|error| error.to_string());
            if let Err(error) = &result {
                progress.fail(error);
            } else {
                progress.finish();
                crate::instance::content::reconcile::spawn_after_change(
                    instance,
                    instances_dir,
                    crate::net::HttpClient::new(),
                );
            }
            widgets::content::DiscoveryState::push_action_result(
                &request.pending,
                widgets::content::discovery::DiscoveryActionResult::Install {
                    request_id: request.request_id,
                    generation: request.generation,
                    project_id: request.project_id,
                    project_title: request.project_title,
                    result,
                },
            );
        });
    }

    fn spawn_active_discovery_search(&mut self) {
        let Some(instance) = self.instances_state.selected_instance().cloned() else {
            return;
        };
        let manifest = self
            .content_manifest
            .as_ref()
            .filter(|(name, _)| name == &instance.name)
            .map(|(_, manifest)| manifest.clone());
        let minecraft_dir = crate::storage::InstancePaths::new(
            self.instance_manager.instances_dir.join(&instance.name),
        )
        .minecraft();
        let meta_dir = self.instance_manager.meta_dir.clone();
        let Some(state) = self.active_discovery_state_mut() else {
            return;
        };
        if state.unavailable_message(&instance).is_some() {
            state.set_unavailable(&instance);
            return;
        }
        let kind = state.kind;
        let query = state.search.query.clone();
        let request = state.begin_search(&instance);
        Self::spawn_discovery_request(
            instance,
            kind,
            query,
            manifest,
            minecraft_dir,
            meta_dir,
            request,
        );
    }

    fn spawn_active_discovery_page(&mut self) {
        let Some(instance) = self.instances_state.selected_instance().cloned() else {
            return;
        };
        let manifest = self
            .content_manifest
            .as_ref()
            .filter(|(name, _)| name == &instance.name)
            .map(|(_, manifest)| manifest.clone());
        let minecraft_dir = crate::storage::InstancePaths::new(
            self.instance_manager.instances_dir.join(&instance.name),
        )
        .minecraft();
        let meta_dir = self.instance_manager.meta_dir.clone();
        let Some(state) = self.active_discovery_state_mut() else {
            return;
        };
        if state.unavailable_message(&instance).is_some() {
            state.set_unavailable(&instance);
            return;
        }
        let kind = state.kind;
        let query = state.search.query.clone();
        let Some(request) = state.begin_next_page() else {
            return;
        };
        Self::spawn_discovery_request(
            instance,
            kind,
            query,
            manifest,
            minecraft_dir,
            meta_dir,
            request,
        );
    }

    fn spawn_discovery_request(
        instance: crate::instance::InstanceConfig,
        kind: crate::instance::ContentKind,
        query: String,
        manifest: Option<crate::instance::ContentManifest>,
        minecraft_dir: std::path::PathBuf,
        meta_dir: std::path::PathBuf,
        request: widgets::content::discovery::DiscoveryRequest,
    ) {
        widgets::content::discovery::spawn_provider_search(
            query,
            widgets::content::discovery::DiscoveryTarget::Content(Box::new(
                widgets::content::discovery::ContentDiscoveryTarget {
                    instance,
                    kind,
                    manifest,
                    minecraft_dir,
                },
            )),
            meta_dir,
            request,
        );
    }

    fn delete_config_profile(&mut self, profile: &str) -> color_eyre::Result<()> {
        let instances = self.instance_manager.load_all();
        for instance in instances
            .into_iter()
            .filter(|instance| instance.config_sync_profile.as_deref() == Some(profile))
        {
            let instance_dir = self.instance_manager.instances_dir.join(&instance.name);
            let mut updated = instance.clone();
            updated.config_sync_profile = crate::instance::config_sync::switch_profile(
                &instance.name,
                instance.config_sync_profile.as_deref(),
                None,
                &self.instance_manager.meta_dir,
                &instance_dir,
            )?;
            self.instance_manager.save(&updated)?;
            self.instances_state
                .replace_instance(&instance.name, updated);
        }

        crate::instance::config_sync::delete_profile(&self.instance_manager.meta_dir, profile)?;
        self.settings_state.remove_profile(profile);
        Ok(())
    }

    fn confirm_content_delete(&mut self, pending: widgets::content::list::PendingContentDelete) {
        let dependents = self
            .content_manifest_for_path(&pending.path)
            .map(|(manifest, relative_path, _)| {
                manifest
                    .dependent_paths(&relative_path)
                    .into_iter()
                    .map(|path| {
                        path.file_stem()
                            .and_then(|name| name.to_str())
                            .unwrap_or("mod")
                            .to_owned()
                    })
                    .collect()
            })
            .unwrap_or_default();
        confirm_popup::set_pending_managed_content_delete(pending.name, pending.path, dependents);
        self.focused = FocusedArea::ConfirmDelete;
    }

    fn orphan_dependencies_after_removing(
        &self,
        path: &std::path::Path,
    ) -> Vec<std::path::PathBuf> {
        self.content_manifest_for_path(path)
            .map(|(manifest, relative_path, minecraft_dir)| {
                manifest
                    .orphaned_dependencies_after_removing(&relative_path)
                    .into_iter()
                    .map(|relative| minecraft_dir.join(relative))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn content_manifest_for_path(
        &self,
        path: &std::path::Path,
    ) -> Option<(
        crate::instance::ContentManifest,
        std::path::PathBuf,
        std::path::PathBuf,
    )> {
        let instance = self.instances_state.selected_instance()?;
        let paths = crate::storage::InstancePaths::new(
            self.instance_manager.instances_dir.join(&instance.name),
        );
        let minecraft_dir = paths.minecraft();
        let relative_path = path.strip_prefix(&minecraft_dir).ok()?.to_owned();
        let manifest = crate::instance::ContentManifest::load(&paths.content_manifest()).ok()?;
        Some((manifest, relative_path, minecraft_dir))
    }

    fn remove_content_path_from_states(&mut self, path: &std::path::Path) {
        self.mods_state.remove_path(path);
        self.resource_packs_state.remove_path(path);
        self.shaders_state.remove_path(path);
        self.world_datapacks_state.remove_path(path);
        self.worlds_state.remove_path(path);
        self.screenshots_state.remove_path(path);
        self.logs_state.remove_path(path);
        self.mods_discovery_state.clear_installed_path(path);
        self.resource_packs_discovery_state
            .clear_installed_path(path);
        self.shaders_discovery_state.clear_installed_path(path);
        self.datapacks_discovery_state.clear_installed_path(path);
        if path
            .parent()
            .and_then(std::path::Path::file_name)
            .is_some_and(|name| name == "datapacks")
            && let Some(world_path) = path.parent().and_then(std::path::Path::parent)
            && let Some(world) = self
                .worlds_state
                .entries
                .iter_mut()
                .find(|world| world.path == world_path)
            && let Some(details) = world.world_details.as_mut()
        {
            details.datapacks = crate::instance::content::worlds::datapack_names(world_path);
        }
    }

    fn remove_content_path_from_manifest(&mut self, path: &std::path::Path) {
        let Some(instance) = self.instances_state.selected_instance() else {
            return;
        };
        let instance_paths = crate::storage::InstancePaths::new(
            self.instance_manager.instances_dir.join(&instance.name),
        );
        let minecraft_dir = instance_paths.minecraft();
        let Ok(relative_path) = path.strip_prefix(&minecraft_dir) else {
            return;
        };
        if let Some((instance_name, manifest)) = self.content_manifest.as_mut()
            && *instance_name == instance.name
        {
            manifest.remove(relative_path);
        }
        if let Err(error) = crate::instance::ContentManifest::update(
            &instance_paths.content_manifest(),
            |manifest| {
                manifest.remove(relative_path);
                Ok(())
            },
        ) {
            tracing::warn!(
                "Failed to remove '{}' from the content manifest: {}",
                relative_path.display(),
                error
            );
        }
    }
}

fn delete_content_path(path: &std::path::Path) -> std::io::Result<()> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
