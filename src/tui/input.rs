// keybindings and input dispatch.
// the general pattern: check which area is focused, give it first crack at the
// keypress, and fall through to global bindings if nobody claimed it.
// vim-style navigation (j/k/g/G) where it makes sense.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, FocusedArea};
use super::widgets::{
    self, WidgetKey, popups::confirm as confirm_popup, popups::import_modpack, popups::new_instance,
};
use crate::tui::error_buffer;

impl App {
    pub(super) fn handle_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
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
                        self.log_overlay_search.pop();
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
                        Some(confirm_popup::ConfirmTarget::Content { name, path }) => {
                            match delete_content_path(&path) {
                                Ok(()) => {
                                    self.remove_content_path_from_states(&path);
                                }
                                Err(e) => {
                                    tracing::error!("Failed to delete content '{}': {}", name, e);
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
            let handled = {
                let state = match self.content_tab {
                    widgets::content::ContentTab::Mods => Some(&mut self.mods_discovery_state),
                    widgets::content::ContentTab::ResourcePacks => {
                        Some(&mut self.resource_packs_discovery_state)
                    }
                    widgets::content::ContentTab::Shaders => {
                        Some(&mut self.shaders_discovery_state)
                    }
                    _ => None,
                };
                if let Some(state) = state {
                    widgets::content::discovery::handle_key(&key_event, state)
                } else {
                    false
                }
            };
            if handled {
                if matches!(
                    key_event.code,
                    KeyCode::Char('j') | KeyCode::Char('k') | KeyCode::Down | KeyCode::Up
                ) {
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
                        confirm_popup::set_pending_content_delete(pending.name, pending.path);
                        self.focused = FocusedArea::ConfirmDelete;
                    }
                    return Ok(());
                }
                if widgets::logs_viewer::handle_key(&key_event, &mut self.logs_state) {
                    return Ok(());
                }
            } else if self.content_tab == widgets::content::ContentTab::Screenshots {
                if key_event.code == KeyCode::Char('d') && !self.screenshots_state.search.active {
                    if let Some(pending) = self.screenshots_state.pending_delete() {
                        confirm_popup::set_pending_content_delete(pending.name, pending.path);
                        self.focused = FocusedArea::ConfirmDelete;
                    }
                    return Ok(());
                }
                if widgets::screenshots_grid::handle_key(&key_event, &mut self.screenshots_state) {
                    return Ok(());
                }
            } else if self.content_tab == widgets::content::ContentTab::Worlds {
                if key_event.code == KeyCode::Char('d') && !self.worlds_state.search.active {
                    if let Some(pending) = self.worlds_state.pending_delete() {
                        confirm_popup::set_pending_content_delete(pending.name, pending.path);
                        self.focused = FocusedArea::ConfirmDelete;
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
                            confirm_popup::set_pending_content_delete(pending.name, pending.path);
                            self.focused = FocusedArea::ConfirmDelete;
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
                                name.pop();
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
                        if self.content_mode == widgets::content::ContentMode::Discover
                            && !matches!(
                                self.content_tab,
                                widgets::content::ContentTab::Mods
                                    | widgets::content::ContentTab::ResourcePacks
                                    | widgets::content::ContentTab::Shaders
                            )
                        {
                            self.content_tab = widgets::content::ContentTab::Mods;
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
                                .join(".minecraft");
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
                            let can_launch = matches!(
                                crate::running::get(&instance.name),
                                None | Some(crate::running::RunState::Crashed(_))
                            );
                            if can_launch {
                                crate::running::remove(&instance.name);
                                crate::instance_logs::clear(&instance.name);
                                self.spawn_launch(instance);
                            }
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
                            crate::running::send_kill(&instance.name);
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
        let (needs_search, search_due) = match self.content_tab {
            widgets::content::ContentTab::Mods => (
                self.mods_discovery_state.needs_search(&instance),
                self.mods_discovery_state.search_due(),
            ),
            widgets::content::ContentTab::ResourcePacks => (
                self.resource_packs_discovery_state.needs_search(&instance),
                self.resource_packs_discovery_state.search_due(),
            ),
            widgets::content::ContentTab::Shaders => (
                self.shaders_discovery_state.needs_search(&instance),
                self.shaders_discovery_state.search_due(),
            ),
            _ => (false, false),
        };
        if needs_search || search_due {
            self.spawn_active_discovery_search();
        } else {
            self.spawn_active_discovery_page();
        }
    }

    fn spawn_active_discovery_search(&mut self) {
        let Some(instance) = self.instances_state.selected_instance().cloned() else {
            return;
        };
        let state = match self.content_tab {
            widgets::content::ContentTab::Mods => &mut self.mods_discovery_state,
            widgets::content::ContentTab::ResourcePacks => &mut self.resource_packs_discovery_state,
            widgets::content::ContentTab::Shaders => &mut self.shaders_discovery_state,
            _ => return,
        };
        let kind = state.kind;
        let query = state.search.query.clone();
        let request = state.begin_search(&instance);
        Self::spawn_discovery_request(instance, kind, query, request);
    }

    fn spawn_active_discovery_page(&mut self) {
        let Some(instance) = self.instances_state.selected_instance().cloned() else {
            return;
        };
        let state = match self.content_tab {
            widgets::content::ContentTab::Mods => &mut self.mods_discovery_state,
            widgets::content::ContentTab::ResourcePacks => &mut self.resource_packs_discovery_state,
            widgets::content::ContentTab::Shaders => &mut self.shaders_discovery_state,
            _ => return,
        };
        let kind = state.kind;
        let query = state.search.query.clone();
        let Some(request) = state.begin_next_page() else {
            return;
        };
        Self::spawn_discovery_request(instance, kind, query, request);
    }

    fn spawn_discovery_request(
        instance: crate::instance::InstanceConfig,
        kind: crate::net::modrinth::DiscoveryKind,
        query: String,
        request: widgets::content::discovery::DiscoveryRequest,
    ) {
        let widgets::content::discovery::DiscoveryRequest {
            generation,
            offset,
            pending,
            stream,
            reconcile,
            loaded_icon_stems,
        } = request;

        tokio::spawn(async move {
            let client = crate::net::HttpClient::new();
            let result = crate::net::modrinth::search_discovery(
                &client,
                kind,
                &query,
                &instance.game_version,
                instance.loader,
                offset,
                widgets::content::discovery::PAGE_SIZE,
            )
            .await;
            let result = match result {
                Ok(results) => {
                    let total_hits = results.total_hits;
                    let received = results.projects.len();
                    let mut returned_stems = std::collections::HashSet::with_capacity(received);
                    let icon_slots = std::sync::Arc::new(tokio::sync::Semaphore::new(8));
                    for mut project in results.projects {
                        returned_stems.insert(project.id.clone());
                        let icon = (!loaded_icon_stems.contains(&project.id))
                            .then(|| project.icon_url.take())
                            .flatten()
                            .map(|url| {
                                (
                                    url,
                                    project.id.clone(),
                                    std::path::PathBuf::from(&project.id),
                                )
                            });
                        if !stream.upsert(widgets::content::discovery::project_entry(project)) {
                            break;
                        }
                        if let Some((url, file_stem, path)) = icon {
                            let client = client.clone();
                            let stream = stream.clone();
                            let icon_slots = icon_slots.clone();
                            tokio::spawn(async move {
                                let Ok(_permit) = icon_slots.acquire_owned().await else {
                                    return;
                                };
                                match client.get_bytes(&url).await {
                                    Ok(bytes) if !bytes.is_empty() => {
                                        stream.send_icon(file_stem, path, bytes);
                                    }
                                    Ok(_) => tracing::debug!(
                                        "Modrinth icon for '{}' was empty; using fallback",
                                        file_stem
                                    ),
                                    Err(error) => tracing::debug!(
                                        "Failed to fetch Modrinth icon for '{}'; using fallback: {}",
                                        file_stem,
                                        error
                                    ),
                                }
                            });
                        }
                    }
                    if reconcile {
                        stream.retain(returned_stems);
                    }
                    Ok(widgets::content::discovery::DiscoveryPageResult {
                        received,
                        total_hits,
                    })
                }
                Err(error) => Err(widgets::content::discovery::DiscoveryPageError {
                    retryable: error.is_retryable(),
                    message: error.to_string(),
                }),
            };
            widgets::content::DiscoveryState::push_result(&pending, generation, offset, result);
        });
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

    fn remove_content_path_from_states(&mut self, path: &std::path::Path) {
        self.mods_state.remove_path(path);
        self.resource_packs_state.remove_path(path);
        self.shaders_state.remove_path(path);
        self.worlds_state.remove_path(path);
        self.screenshots_state.remove_path(path);
        self.logs_state.remove_path(path);
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
