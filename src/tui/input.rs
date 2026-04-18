use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::app::{App, FocusedArea};
use super::widgets::{self, popups::confirm as confirm_popup, popups::import_modpack, popups::new_instance, WidgetKey};
use crate::tui::error_buffer;

impl App {
    pub(super) fn handle_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        if self.focused == FocusedArea::OverviewExpanded {
            if self.log_overlay_search.active {
                match key_event.code {
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
                    let name = confirm_popup::pending_delete_name();
                    if !name.is_empty() {
                        match self.instance_manager.delete(&name) {
                            Ok(_) => {
                                self.instances_state.remove_instance(&name);
                            }
                            Err(e) => {
                                tracing::error!("Failed to delete instance '{}': {}", name, e);
                            }
                        }
                    }
                    confirm_popup::clear_pending();
                    self.focused = FocusedArea::Instances;
                    return Ok(());
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    confirm_popup::clear_pending();
                    self.focused = FocusedArea::Instances;
                    return Ok(());
                }
                _ => {
                    return Ok(());
                }
            }
        }

        if self.focused == FocusedArea::Content {
            if self.content_tab == widgets::content::ContentTab::Logs {
                if widgets::logs_viewer::handle_key(&key_event, &mut self.logs_state) {
                    return Ok(());
                }
            } else if self.content_tab == widgets::content::ContentTab::Screenshots {
                if widgets::screenshots_grid::handle_key(&key_event, &mut self.screenshots_state) {
                    return Ok(());
                }
            } else if self.content_tab == widgets::content::ContentTab::Worlds {
                if widgets::content::list::handle_key_no_toggle(&key_event, &mut self.worlds_state) {
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
                    if widgets::content::list::handle_key(&key_event, state) {
                        return Ok(());
                    }
                }
            }
        }

        if self.focused == FocusedArea::Account
            && widgets::account::handle_key(&key_event, &mut self.account_state)
        {
            return Ok(());
        }

        if self.focused == FocusedArea::Settings {
            match widgets::details::handle_key(
                &key_event,
                self.instances_state.selected_instance(),
                &self.instance_manager.instances_dir,
            ) {
                widgets::details::SettingsAction::EditInstance(path)
                | widgets::details::SettingsAction::EditGlobal(path) => {
                    self.pending_editor = Some(path);
                    return Ok(());
                }
                widgets::details::SettingsAction::ToggleDesktop => {
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
                widgets::details::SettingsAction::None => {}
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
                if self.focused == FocusedArea::Instances && self.instances_state.renaming.is_some() {
                    match key_event.code {
                        KeyCode::Enter => {
                            let new_name = self.instances_state.renaming.take().unwrap_or_default();
                            if let Some(inst) = self.instances_state.selected_instance() {
                                let old_name = inst.name.clone();
                                match self.instance_manager.rename(&old_name, &new_name) {
                                    Ok(()) => {
                                        if let Some(inst) = self
                                            .instances_state
                                            .instances
                                            .iter_mut()
                                            .find(|i| i.name == old_name)
                                        {
                                            inst.name = new_name.trim().to_owned();
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Rename failed: {}", e);
                                    }
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
                    KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right
                        if self.focused == FocusedArea::Content =>
                    {
                        self.content_tab = self.content_tab.next();
                    }
                    KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left
                        if self.focused == FocusedArea::Content =>
                    {
                        self.content_tab = self.content_tab.previous();
                    }
                    KeyCode::Char('d')
                        if self.focused == FocusedArea::Instances
                            && !self.instances_state.search.active =>
                    {
                        if let Some(instance) = self.instances_state.selected_instance() {
                            let name = instance.name.clone();
                            confirm_popup::set_pending_delete(&name);
                            self.focused = FocusedArea::ConfirmDelete;
                        }
                    }
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
                            if let Err(e) = open::that(&dir) {
                                tracing::error!("Failed to open instance directory: {}", e);
                            }
                        }
                    }
                    KeyCode::Enter
                        if self.focused == FocusedArea::Instances
                            && !self.instances_state.search.active =>
                    {
                        self.focused = FocusedArea::Content;
                    }
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
}
