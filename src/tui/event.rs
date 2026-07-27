use color_eyre::eyre::Context;
use crossterm::event::{self, Event};
use ratatui::crossterm::event::KeyEventKind;
use std::time::Duration;

use super::Tui;
use super::app::{App, PENDING_INSTANCES};
use super::widgets::{self, popups::import_modpack, popups::new_instance};
use crate::feedback::errors as error_buffer;
use crate::feedback::progress;
use crate::instance::InstanceManager;

impl App {
    /// main loop: poll async results and input at ~60Hz, drawing only when state changes
    pub async fn run(&mut self, terminal: &mut Tui) -> color_eyre::Result<()> {
        let mut last_draw = std::time::Instant::now()
            .checked_sub(Duration::from_secs(1))
            .unwrap_or_else(std::time::Instant::now);
        while !self.exit {
            let redraw_requested = crate::feedback::take_redraw_request();
            // check if any popup wizard finished and wants to create/import
            if let Some(params) = new_instance::take_result() {
                self.spawn_create(params);
            }

            if let Some(result) = import_modpack::take_result() {
                self.spawn_import(result);
            }

            self.dismiss_expired_errors();

            // drain all the channels from background tasks.
            // every content type has its own pending queue because they each
            // get scanned/loaded on separate tokio tasks
            self.drain_pending_instances();
            self.drain_pending_last_played();
            let mut local_streamed = false;
            let mut content_changed = false;
            let mut toggles = Vec::new();
            for (local, discovery) in [
                (&mut self.mods_state, &mut self.mods_discovery_state),
                (
                    &mut self.resource_packs_state,
                    &mut self.resource_packs_discovery_state,
                ),
                (&mut self.shaders_state, &mut self.shaders_discovery_state),
            ] {
                local_streamed |= local.drain_pending();
                let update = local.drain_watcher();
                content_changed |= update.requires_reconcile;
                toggles.extend(update.toggles);
                local.drain_provider_icons();
                local.request_image_loads(&self.picker);
                local.drain_image_loads(&self.picker);
                discovery.drain_pending();
                discovery.list.drain_pending();
                discovery.list.request_image_loads(&self.picker);
                discovery.list.drain_image_loads(&self.picker);
            }
            self.worlds_state.drain_pending();
            self.worlds_state.drain_watcher();
            self.worlds_state.request_image_loads(&self.picker);
            self.worlds_state.drain_image_loads(&self.picker);
            self.logs_state.drain_pending();
            self.logs_state.try_rescan();
            self.account_state.drain_auth_result();
            widgets::account::drain_device_code(&mut self.account_state);
            self.screenshots_state.drain_pending_entries();
            self.screenshots_state.request_visible_loads();
            self.create_screenshot_protocols();
            if !toggles.is_empty() {
                content_changed |= self.persist_content_toggles(&toggles);
            }
            if content_changed
                && let Some(instance) = self.instances_state.selected_instance()
                && let Ok(mut results) =
                    crate::instance::content::reconcile::PENDING_RECONCILIATIONS.lock()
            {
                results.retain(|result| result.instance_name != instance.name);
            }
            self.drain_content_reconciliation();
            self.ensure_content_reconciliation(content_changed);
            if local_streamed {
                self.apply_cached_content_manifest();
            }
            self.ensure_provider_conflict_popup();
            self.ensure_active_discovery_loaded();
            let progress_active = progress::is_active();
            let spinner_active = progress_active || crate::instance::runtime::has_active();
            if spinner_active {
                // only advance the spinner every 8 ticks to keep it readable
                self.throbber_tick = self.throbber_tick.wrapping_add(1);
                if self.throbber_tick.is_multiple_of(8) {
                    self.throbber_state.calc_next();
                }
            }

            let input_changed = self.handle_events().wrap_err("handle events failed")?;
            let continuously_animated = spinner_active || error_buffer::has_errors();
            let safety_refresh = last_draw.elapsed() >= Duration::from_secs(1);
            if input_changed || continuously_animated || safety_refresh || redraw_requested {
                terminal.draw(|frame| self.render_frame(frame))?;
                last_draw = std::time::Instant::now();
            }

            if let Some(path) = self.pending_editor.take()
                && Self::run_editor(terminal, &path)
            {
                self.reload_edited_config(&path);
            }
        }
        Ok(())
    }

    fn persist_content_toggles(
        &mut self,
        toggles: &[widgets::content::list::ContentToggle],
    ) -> bool {
        let Some(instance) = self.instances_state.selected_instance() else {
            return true;
        };
        let instance_name = instance.name.clone();
        let paths = crate::storage::InstancePaths::new(
            self.instance_manager.instances_dir.join(&instance_name),
        );
        let minecraft_dir = paths.minecraft();
        let updated =
            crate::instance::ContentManifest::update(&paths.content_manifest(), |manifest| {
                let mut complete = true;
                for toggle in toggles {
                    let Ok(old_path) = toggle.old_path.strip_prefix(&minecraft_dir) else {
                        complete = false;
                        continue;
                    };
                    let Ok(new_path) = toggle.new_path.strip_prefix(&minecraft_dir) else {
                        complete = false;
                        continue;
                    };
                    complete &= manifest.rename_record(old_path, new_path, toggle.enabled);
                }
                Ok((manifest.clone(), complete))
            });
        let (manifest, complete) = match updated {
            Ok(updated) => updated,
            Err(error) => {
                tracing::warn!("Failed to update toggled content metadata: {error}");
                return true;
            }
        };

        self.mods_discovery_state
            .refresh_installed_manifest(&manifest, &minecraft_dir);
        self.resource_packs_discovery_state
            .refresh_installed_manifest(&manifest, &minecraft_dir);
        self.shaders_discovery_state
            .refresh_installed_manifest(&manifest, &minecraft_dir);
        self.content_manifest = Some((instance_name, manifest));
        !complete
    }

    fn ensure_content_reconciliation(&mut self, changed: bool) {
        let Some(instance) = self.instances_state.selected_instance().cloned() else {
            self.reconciliation_for = None;
            self.content_manifest = None;
            return;
        };
        if !changed && self.reconciliation_for.as_deref() == Some(instance.name.as_str()) {
            return;
        }
        let instance_changed = self.reconciliation_for.as_deref() != Some(instance.name.as_str());
        if instance_changed {
            self.provider_conflict = None;
            self.dismissed_provider_conflicts.clear();
            self.content_manifest = None;
            for discovery in [
                &mut self.mods_discovery_state,
                &mut self.resource_packs_discovery_state,
                &mut self.shaders_discovery_state,
            ] {
                discovery.refresh_installed_manifest(
                    &crate::instance::ContentManifest::default(),
                    &crate::storage::InstancePaths::new(
                        self.instance_manager.instances_dir.join(&instance.name),
                    )
                    .minecraft(),
                );
            }
        }
        self.reconciliation_for = Some(instance.name.clone());
        if changed {
            crate::instance::content::reconcile::spawn_after_change(
                instance,
                self.instance_manager.instances_dir.clone(),
                crate::net::HttpClient::new(),
            );
        } else {
            crate::instance::content::reconcile::spawn(
                instance,
                self.instance_manager.instances_dir.clone(),
                crate::net::HttpClient::new(),
            );
        }
    }

    fn drain_content_reconciliation(&mut self) {
        let Some(selected) = self.instances_state.selected_instance() else {
            return;
        };
        let result = match crate::instance::content::reconcile::PENDING_RECONCILIATIONS.lock() {
            Ok(mut results) => results
                .iter()
                .position(|result| result.instance_name == selected.name)
                .map(|index| results.remove(index)),
            Err(_) => return,
        };
        let Some(result) = result else {
            return;
        };
        self.reconciliation_for = Some(result.instance_name.clone());
        if let Some(error) = result.error {
            tracing::warn!(
                "Content reconciliation for {} was incomplete: {}",
                result.instance_name,
                error
            );
        }
        let minecraft_dir = crate::storage::InstancePaths::new(
            self.instance_manager.instances_dir.join(&selected.name),
        )
        .minecraft();
        self.mods_state.apply_manifest(
            &result.manifest,
            &minecraft_dir,
            crate::instance::ContentKind::Mod,
        );
        self.resource_packs_state.apply_manifest(
            &result.manifest,
            &minecraft_dir,
            crate::instance::ContentKind::ResourcePack,
        );
        self.shaders_state.apply_manifest(
            &result.manifest,
            &minecraft_dir,
            crate::instance::ContentKind::Shader,
        );
        self.mods_discovery_state
            .refresh_installed_manifest(&result.manifest, &minecraft_dir);
        self.resource_packs_discovery_state
            .refresh_installed_manifest(&result.manifest, &minecraft_dir);
        self.shaders_discovery_state
            .refresh_installed_manifest(&result.manifest, &minecraft_dir);
        self.content_manifest = Some((result.instance_name, result.manifest));
    }

    fn apply_cached_content_manifest(&mut self) {
        let Some((instance_name, manifest)) = &self.content_manifest else {
            return;
        };
        if self
            .instances_state
            .selected_instance()
            .is_none_or(|instance| instance.name != *instance_name)
        {
            return;
        }
        let minecraft_dir = crate::storage::InstancePaths::new(
            self.instance_manager.instances_dir.join(instance_name),
        )
        .minecraft();
        self.mods_state
            .apply_manifest(manifest, &minecraft_dir, crate::instance::ContentKind::Mod);
        self.resource_packs_state.apply_manifest(
            manifest,
            &minecraft_dir,
            crate::instance::ContentKind::ResourcePack,
        );
        self.shaders_state.apply_manifest(
            manifest,
            &minecraft_dir,
            crate::instance::ContentKind::Shader,
        );
    }

    fn ensure_provider_conflict_popup(&mut self) {
        if !crate::config::SETTINGS.content.ask_on_provider_conflict
            || self.focused != super::app::FocusedArea::Content
            || self.provider_conflict.is_some()
        {
            return;
        }
        let Some((instance_name, manifest)) = &self.content_manifest else {
            return;
        };
        if self
            .instances_state
            .selected_instance()
            .is_none_or(|instance| instance.name != *instance_name)
        {
            return;
        }
        self.provider_conflict = manifest.files.iter().find_map(|record| {
            if self
                .dismissed_provider_conflicts
                .contains(&record.relative_path)
            {
                return None;
            }
            let crate::instance::Resolution::Ambiguous { candidates } = &record.resolution else {
                return None;
            };
            Some(super::app::ProviderConflictState {
                relative_path: record.relative_path.clone(),
                candidates: candidates.clone(),
                selected: 0,
            })
        });
    }

    // polls for input with a 16ms timeout (~60fps). only key presses are handled,
    // releases and repeats are ignored thanks to the enhanced keyboard protocol
    fn handle_events(&mut self) -> color_eyre::Result<bool> {
        match crossterm::event::poll(Duration::from_millis(16)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(key_event)) if key_event.kind == KeyEventKind::Press => {
                    self.handle_key_event(key_event)
                        .wrap_err_with(|| format!("handling key event failed:\n{key_event:#?}"))?;
                    Ok(true)
                }
                Ok(Event::Mouse(mouse_event)) => {
                    self.handle_mouse_event(mouse_event);
                    Ok(true)
                }
                Ok(_) => Ok(true),
                Err(e) => {
                    tracing::error!("Event read error: {}", e);
                    Ok(false)
                }
            },
            Ok(false) => Ok(false),
            Err(e) => {
                tracing::error!("Event poll error: {}", e);
                Ok(false)
            }
        }
    }

    fn spawn_create(&self, params: new_instance::WizardParams) {
        let instances_dir = self.instance_manager.instances_dir.clone();
        let meta_dir = crate::config::SETTINGS.paths.resolve_meta_dir();
        let pending_instances = PENDING_INSTANCES.clone();

        tokio::spawn(async move {
            progress::set_action(format!("Creating instance '{}'...", params.name));
            progress::set_sub_action(format!("{} {}", params.game_version, params.loader));

            let manager = InstanceManager::new(instances_dir, meta_dir);
            match manager
                .create(
                    &params.name,
                    &params.game_version,
                    params.loader,
                    params.loader_version.as_deref(),
                )
                .await
            {
                Ok(config) => {
                    if let Ok(mut pending) = pending_instances.lock() {
                        pending.push(config);
                        crate::feedback::request_redraw();
                    }
                }
                Err(e) => {
                    progress::clear();
                    error_buffer::push_error(error_buffer::ErrorEvent {
                        id: 0,
                        level: tracing::Level::ERROR,
                        message: format!("Failed to create instance '{}': {e}", params.name),
                        pushed_at: std::time::Instant::now(),
                    });
                }
            }
        });
    }

    fn spawn_import(&self, result: import_modpack::ImportResult) {
        let instances_dir = self.instance_manager.instances_dir.clone();
        let meta_dir = crate::config::SETTINGS.paths.resolve_meta_dir();
        let pending_instances = PENDING_INSTANCES.clone();

        tokio::spawn(async move {
            let manager = InstanceManager::new(instances_dir, meta_dir);
            match crate::instance::import::execute_import(&result.summary, &manager).await {
                Ok(config) => {
                    if let Ok(mut pending) = pending_instances.lock() {
                        pending.push(config);
                        crate::feedback::request_redraw();
                    }
                }
                Err(e) => {
                    crate::feedback::progress::clear();
                    error_buffer::push_error(error_buffer::ErrorEvent {
                        id: 0,
                        level: tracing::Level::ERROR,
                        message: format!("Import failed: {e}"),
                        pushed_at: std::time::Instant::now(),
                    });
                }
            }
        });
    }

    // spawns $EDITOR/$VISUAL to edit a file. for terminal editors (vim, nano, etc)
    // gotta leave the alternate screen and restore it after, otherwise the
    // editor fights with ratatui for the terminal. GUI editors just get spawned detached.
    fn run_editor(terminal: &mut ratatui::DefaultTerminal, path: &std::path::Path) -> bool {
        use ratatui::crossterm::{
            ExecutableCommand,
            event::{DisableMouseCapture, EnableMouseCapture},
            terminal::{
                EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
            },
        };
        use std::io::stdout;

        let default_editor = if cfg!(windows) { "notepad" } else { "vi" };
        let editor = std::env::var("EDITOR")
            .or_else(|_| std::env::var("VISUAL"))
            .unwrap_or_else(|_| default_editor.to_owned());

        let is_tui_editor = editor_runs_in_terminal(&editor);

        if is_tui_editor {
            let _ = stdout().execute(DisableMouseCapture);
            let _ = stdout().execute(LeaveAlternateScreen);
            let _ = disable_raw_mode();

            let result = std::process::Command::new(&editor)
                .arg(path)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status();

            let _ = stdout().execute(EnterAlternateScreen);
            let _ = stdout().execute(EnableMouseCapture);
            let _ = enable_raw_mode();
            let _ = terminal.clear();

            if let Err(e) = result {
                tracing::error!("Failed to open editor: {}", e);
                return false;
            }
            true
        } else {
            if let Err(e) = std::process::Command::new(&editor)
                .arg(path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                tracing::error!("Failed to open editor: {}", e);
                return false;
            }
            false
        }
    }

    fn reload_edited_config(&mut self, path: &std::path::Path) {
        if path.file_name().and_then(|n| n.to_str()) != Some("instance.json") {
            return;
        }

        let Some(name) = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
        else {
            return;
        };

        match self.instance_manager.load_one(name) {
            Ok(config) => {
                self.instances_state.replace_instance(name, config);
            }
            Err(e) => {
                tracing::error!("Failed to reload edited instance '{}': {}", name, e);
                error_buffer::push_error(error_buffer::ErrorEvent {
                    id: 0,
                    level: tracing::Level::ERROR,
                    message: format!("Failed to reload edited instance '{name}': {e}"),
                    pushed_at: std::time::Instant::now(),
                });
            }
        }
    }

    pub(super) fn spawn_launch(&self, instance: crate::instance::InstanceConfig) {
        use crate::instance::launch;
        use crate::instance::runtime;

        let instance = match self.instance_manager.load_one(&instance.name) {
            Ok(config) => config,
            Err(e) => {
                error_buffer::push_error(error_buffer::ErrorEvent {
                    id: 0,
                    level: tracing::Level::ERROR,
                    message: format!("Failed to load instance '{}': {e}", instance.name),
                    pushed_at: std::time::Instant::now(),
                });
                return;
            }
        };

        runtime::set_state(&instance.name, runtime::RunState::Authenticating);

        let instances_dir = self.instance_manager.instances_dir.clone();
        let meta_dir = self.instance_manager.meta_dir.clone();

        tokio::spawn(async move {
            if let Err(e) = launch::launch(&instance, &instances_dir, &meta_dir).await {
                tracing::error!("Failed to launch '{}': {}", instance.name, e);
                runtime::remove(&instance.name);
            }
        });
    }

    // pops errors from the front of the queue once they've been visible long enough.
    // loops because multiple errors could expire in the same frame
    fn dismiss_expired_errors(&self) {
        use crate::config::SETTINGS;
        loop {
            match error_buffer::peek_error() {
                Some(event)
                    if event.pushed_at.elapsed().as_millis()
                        >= SETTINGS.ui.error_auto_dismiss_ms as u128 =>
                {
                    let _ = error_buffer::pop_error();
                }
                _ => break,
            }
        }
    }

    fn drain_pending_instances(&mut self) {
        if let Ok(mut pending) = PENDING_INSTANCES.lock() {
            for config in pending.drain(..) {
                self.instances_state.add_instance(config);
            }
        }
    }

    fn drain_pending_last_played(&mut self) {
        for (name, time) in crate::instance::runtime::drain_last_played() {
            for inst in &mut self.instances_state.instances {
                if inst.name == name {
                    inst.last_played = Some(time);
                    break;
                }
            }
        }
    }

    pub(super) fn create_screenshot_protocols(&mut self) {
        let pending = self.screenshots_state.take_pending_images();
        for (idx, img) in pending {
            let proto = self.picker.new_resize_protocol(img);
            self.screenshots_state.set_protocol(idx, proto);
        }
    }
}

fn editor_runs_in_terminal(editor: &str) -> bool {
    let editor_name = std::path::Path::new(editor)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or(editor);
    matches!(
        editor_name,
        "vi" | "vim"
            | "nvim"
            | "neovim"
            | "nano"
            | "micro"
            | "helix"
            | "hx"
            | "emacs"
            | "ne"
            | "joe"
            | "mcedit"
    )
}

#[cfg(test)]
#[path = "tests/event.rs"]
mod tests;
