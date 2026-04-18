use color_eyre::eyre::Context;
use crossterm::event::{self, Event};
use ratatui::crossterm::event::KeyEventKind;
use std::time::Duration;

use super::app::{App, PENDING_INSTANCES};
use super::widgets::{self, popups::import_modpack, popups::new_instance};
use super::Tui;
use crate::instance::InstanceManager;
use crate::tui::error_buffer;
use crate::tui::progress;

impl App {
    pub async fn run(&mut self, terminal: &mut Tui) -> color_eyre::Result<()> {
        while !self.exit {
            if let Some(params) = new_instance::take_result() {
                self.spawn_create(params);
            }

            if let Some(result) = import_modpack::take_result() {
                self.spawn_import(result);
            }

            self.dismiss_expired_errors();

            self.drain_pending_instances();
            self.drain_pending_last_played();
            self.mods_state.drain_pending();
            self.mods_state.try_rescan();
            self.resource_packs_state.drain_pending();
            self.resource_packs_state.try_rescan();
            self.shaders_state.drain_pending();
            self.shaders_state.try_rescan();
            self.worlds_state.drain_pending();
            self.worlds_state.try_rescan();
            self.logs_state.drain_pending();
            self.logs_state.try_rescan();
            self.account_state.drain_auth_result();
            widgets::account::drain_device_code(&mut self.account_state);
            self.screenshots_state.drain_pending_entries();
            self.screenshots_state.request_visible_loads();
            self.create_screenshot_protocols();
            self.throbber_tick = self.throbber_tick.wrapping_add(1);
            if self.throbber_tick.is_multiple_of(8) {
                self.throbber_state.calc_next();
            }

            terminal.draw(|frame| self.render_frame(frame))?;
            self.handle_events().wrap_err("handle events failed")?;

            if let Some(path) = self.pending_editor.take() {
                Self::run_editor(terminal, &path);
            }
        }
        Ok(())
    }

    fn handle_events(&mut self) -> color_eyre::Result<()> {
        match crossterm::event::poll(Duration::from_millis(16)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(key_event)) if key_event.kind == KeyEventKind::Press => self
                    .handle_key_event(key_event)
                    .wrap_err_with(|| format!("handling key event failed:\n{key_event:#?}")),
                Ok(_) => Ok(()),
                Err(e) => {
                    tracing::error!("Event read error: {}", e);
                    Ok(())
                }
            },
            Ok(false) => Ok(()),
            Err(e) => {
                tracing::error!("Event poll error: {}", e);
                Ok(())
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
                    }
                }
                Err(e) => {
                    crate::tui::progress::clear();
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

    fn run_editor(terminal: &mut ratatui::DefaultTerminal, path: &std::path::Path) {
        use ratatui::crossterm::{
            terminal::{
                disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
            },
            ExecutableCommand,
        };
        use std::io::stdout;

        let default_editor = if cfg!(windows) { "notepad" } else { "vi" };
        let editor = std::env::var("EDITOR")
            .or_else(|_| std::env::var("VISUAL"))
            .unwrap_or_else(|_| default_editor.to_owned());

        let editor_name = std::path::Path::new(&editor)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&editor);
        let is_tui_editor = matches!(
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
        );

        if is_tui_editor {
            let _ = stdout().execute(LeaveAlternateScreen);
            let _ = disable_raw_mode();

            let result = std::process::Command::new(&editor)
                .arg(path)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status();

            let _ = stdout().execute(EnterAlternateScreen);
            let _ = enable_raw_mode();
            let _ = terminal.clear();

            if let Err(e) = result {
                tracing::error!("Failed to open editor: {}", e);
            }
        } else { if let Err(e) = std::process::Command::new(&editor)
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn() {
            tracing::error!("Failed to open editor: {}", e);
        }}
    }

    pub(super) fn spawn_launch(&self, instance: crate::instance::InstanceConfig) {
        use crate::instance::launch;
        use crate::running;

        running::set_state(&instance.name, running::RunState::Authenticating);

        let instances_dir = self.instance_manager.instances_dir.clone();
        let meta_dir = self.instance_manager.meta_dir.clone();

        tokio::spawn(async move {
            if let Err(e) = launch::launch(&instance, &instances_dir, &meta_dir).await {
                tracing::error!("Failed to launch '{}': {}", instance.name, e);
                running::remove(&instance.name);
            }
        });
    }

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
        for (name, time) in crate::running::drain_last_played() {
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
