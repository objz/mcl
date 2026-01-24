use super::widgets::popups::new_instance;
use super::{
    widgets::{self, profiles, WidgetKey},
    Tui,
};
use crate::instance::{InstanceConfig, InstanceManager};
use crate::tui::error_buffer;
use crate::tui::progress;
use crate::tui::widgets::popups::confirm as confirm_popup;
use crate::tui::widgets::popups::confirm::{confirm_popup_area, ConfirmPopup};
use crate::tui::widgets::popups::error::{popup_area, ErrorPopup};
use color_eyre::eyre::Context;
use crossterm::event::{self, Event};
use once_cell::sync::Lazy;
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    layout::{Constraint, Direction, Layout},
    Frame,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tachyonfx::{fx, Effect, EffectRenderer, Interpolation, Motion};

static PENDING_INSTANCES: Lazy<Arc<Mutex<Vec<InstanceConfig>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

pub struct App {
    exit: bool,
    focused: FocusedArea,
    pre_overlay_focused: FocusedArea,
    content_tab: widgets::content::ContentTab,
    profiles_state: profiles::State,
    mods_state: widgets::content_list::ContentListState,
    resource_packs_state: widgets::content_list::ContentListState,
    shaders_state: widgets::content_list::ContentListState,
    worlds_state: widgets::content_list::ContentListState,
    screenshots_state: widgets::screenshots_grid::ScreenshotsState,
    logs_state: widgets::logs_viewer::LogsState,
    account_state: widgets::account::AccountState,
    picker: ratatui_image::picker::Picker,
    instance_manager: InstanceManager,
    log_overlay_scroll: usize,
    log_overlay_max_scroll: usize,
    log_overlay_search: widgets::search::SearchState,
    log_overlay_scrollbar: ratatui::widgets::ScrollbarState,
    throbber_state: throbber_widgets_tui::ThrobberState,
    throbber_tick: u8,
    error_effects: HashMap<u64, ErrorEffectState>,
    pending_editor: Option<std::path::PathBuf>,
}

enum ErrorEffectState {
    SlidingIn(Effect),
    Idle,
    FadingOut(Effect),
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub enum FocusedArea {
    #[default]
    Profiles,
    Content,
    Account,
    Settings,
    Overview,
    OverviewExpanded,
    Popup,
    ErrorPopup,
    ConfirmDelete,
}

impl App {
    pub fn new(picker: ratatui_image::picker::Picker) -> Self {
        let instances_dir = crate::config::SETTINGS.paths.resolve_instances_dir();
        let meta_dir = crate::config::SETTINGS.paths.resolve_meta_dir();

        match std::fs::create_dir_all(&instances_dir) {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("Failed to create instances dir: {}", e);
            }
        }

        match std::fs::create_dir_all(&meta_dir) {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("Failed to create meta dir: {}", e);
            }
        }

        let manager = InstanceManager::new(instances_dir, meta_dir);
        let instances = manager.load_all();
        let profiles_state = profiles::State::with_instances(instances);

        App {
            exit: false,
            focused: FocusedArea::default(),
            pre_overlay_focused: FocusedArea::default(),
            content_tab: widgets::content::ContentTab::default(),
            profiles_state,
            mods_state: widgets::content_list::ContentListState::default(),
            resource_packs_state: widgets::content_list::ContentListState::default(),
            shaders_state: widgets::content_list::ContentListState::default(),
            worlds_state: widgets::content_list::ContentListState::default(),
            logs_state: widgets::logs_viewer::LogsState::default(),
            account_state: widgets::account::AccountState::default(),
            screenshots_state: {
                let mut s = widgets::screenshots_grid::ScreenshotsState::default();
                s.font_size = picker.font_size();
                s
            },
            picker,
            instance_manager: manager,
            log_overlay_scroll: 0,
            log_overlay_max_scroll: 0,
            log_overlay_search: widgets::search::SearchState::default(),
            log_overlay_scrollbar: ratatui::widgets::ScrollbarState::default(),
            throbber_state: throbber_widgets_tui::ThrobberState::default(),
            throbber_tick: 0,
            error_effects: HashMap::new(),
            pending_editor: None,
        }
    }
}

impl App {
    pub async fn run(&mut self, terminal: &mut Tui) -> color_eyre::Result<()> {
        while !self.exit {
            if let Some(params) = new_instance::take_result() {
                self.spawn_create(params);
            }

            self.dismiss_expired_errors();

            self.drain_pending_instances();
            self.drain_pending_last_played();
            self.mods_state.drain_pending();
            self.resource_packs_state.drain_pending();
            self.shaders_state.drain_pending();
            self.worlds_state.drain_pending();
            self.logs_state.drain_pending();
            self.logs_state.try_rescan();
            self.account_state.drain_auth_result();
            widgets::account::drain_device_code(&mut self.account_state);
            self.screenshots_state.drain_pending_entries();
            self.screenshots_state.request_visible_loads();
            self.create_screenshot_protocols();
            self.throbber_tick = self.throbber_tick.wrapping_add(1);
            if self.throbber_tick % 8 == 0 {
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

    fn render_frame(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20), // Instances
                Constraint::Percentage(80), // Main content
            ])
            .split(frame.area());

        widgets::profiles::render(frame, chunks[0], self.focused, &mut self.profiles_state);

        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title
                Constraint::Min(10),   // Main Content
                Constraint::Length(5), // Bottom panel
            ])
            .split(chunks[1]);

        widgets::content::title(
            frame,
            main_chunks[0],
            self.focused,
            self.profiles_state.selected_instance(),
            &mut self.throbber_state,
        );
        widgets::content::render(
            frame,
            main_chunks[1],
            self.focused,
            self.content_tab,
            self.profiles_state.selected_instance(),
            &mut self.mods_state,
            &mut self.resource_packs_state,
            &mut self.shaders_state,
            &mut self.worlds_state,
            &mut self.screenshots_state,
            &mut self.logs_state,
            &self.instance_manager.instances_dir,
        );

        let bottom_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Percentage(40),
                Constraint::Percentage(30),
            ])
            .split(main_chunks[2]);

        widgets::account::render(
            frame,
            bottom_chunks[0],
            self.focused,
            &mut self.account_state,
        );
        widgets::details::render(
            frame,
            bottom_chunks[1],
            self.focused,
            self.profiles_state.selected_instance(),
            &self.instance_manager.instances_dir,
        );
        widgets::status::render(
            frame,
            bottom_chunks[2],
            self.focused,
            &mut self.throbber_state,
        );

        if self.focused == FocusedArea::OverviewExpanded {
            self.render_log_overlay(frame);
        }

        let all_errors = error_buffer::peek_all_errors();
        self.sync_error_effects(&all_errors);
        let mut next_y: u16 = 1;
        for event in all_errors {
            let elapsed_ms = event.pushed_at.elapsed().as_millis();
            match popup_area(frame.area(), &event.message, next_y, elapsed_ms) {
                Some(area) => {
                    next_y = next_y.saturating_add(area.height + 1);
                    frame.render_widget(ErrorPopup::new(event.clone()), area);
                    self.render_error_effect(frame, area, &event, elapsed_ms);
                }
                None => {}
            }
        }

        if self.profiles_state.show_popup {
            let area = new_instance::popup_rect(frame.area());
            new_instance::render(frame, area, self.focused);
        }

        if self.focused == FocusedArea::ConfirmDelete {
            let name = confirm_popup::pending_delete_name();
            if !name.is_empty() {
                let area = confirm_popup_area(frame.area(), &name);
                frame.render_widget(ConfirmPopup::new(&name), area);
            }
        }
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

    fn handle_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
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
                                self.profiles_state.remove_instance(&name);
                            }
                            Err(e) => {
                                tracing::error!("Failed to delete instance '{}': {}", name, e);
                            }
                        }
                    }
                    confirm_popup::clear_pending();
                    self.focused = FocusedArea::Profiles;
                    return Ok(());
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    confirm_popup::clear_pending();
                    self.focused = FocusedArea::Profiles;
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
                if widgets::content_list::handle_key_no_toggle(&key_event, &mut self.worlds_state) {
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
                    if widgets::content_list::handle_key(&key_event, state) {
                        return Ok(());
                    }
                }
            }
        }

        if self.focused == FocusedArea::Account {
            if widgets::account::handle_key(&key_event, &mut self.account_state) {
                return Ok(());
            }
        }

        if self.focused == FocusedArea::Settings {
            match widgets::details::handle_key(
                &key_event,
                self.profiles_state.selected_instance(),
                &self.instance_manager.instances_dir,
            ) {
                widgets::details::SettingsAction::EditInstance(path)
                | widgets::details::SettingsAction::EditGlobal(path) => {
                    self.pending_editor = Some(path);
                    return Ok(());
                }
                widgets::details::SettingsAction::None => {}
            }
        }

        match self.focused {
            FocusedArea::Popup => {
                new_instance::handle_key(&key_event, &mut self.profiles_state);
            }
            _ => {
                if self.focused == FocusedArea::Profiles && self.profiles_state.renaming.is_some() {
                    match key_event.code {
                        KeyCode::Enter => {
                            let new_name = self.profiles_state.renaming.take().unwrap_or_default();
                            if let Some(inst) = self.profiles_state.selected_instance() {
                                let old_name = inst.name.clone();
                                match self.instance_manager.rename(&old_name, &new_name) {
                                    Ok(()) => {
                                        if let Some(inst) = self
                                            .profiles_state
                                            .instances
                                            .iter_mut()
                                            .find(|i| i.name == old_name)
                                        {
                                            inst.name = new_name.trim().to_string();
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Rename failed: {}", e);
                                    }
                                }
                            }
                        }
                        KeyCode::Esc => {
                            self.profiles_state.renaming = None;
                        }
                        KeyCode::Backspace => {
                            if let Some(ref mut name) = self.profiles_state.renaming {
                                name.pop();
                            }
                        }
                        KeyCode::Char(c) => {
                            if let Some(ref mut name) = self.profiles_state.renaming {
                                name.push(c);
                            }
                        }
                        _ => {}
                    }
                    return Ok(());
                }

                if self.focused == FocusedArea::Profiles && self.profiles_state.search.active {
                    self.profiles_state.handle_key(&key_event);
                    return Ok(());
                }

                match key_event.code {
                    KeyCode::Char('q') => self.exit = true,
                    KeyCode::Char('P') => self.focused = FocusedArea::Profiles,
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
                        if self.focused == FocusedArea::Profiles
                            && !self.profiles_state.search.active =>
                    {
                        if let Some(instance) = self.profiles_state.selected_instance() {
                            let name = instance.name.clone();
                            confirm_popup::set_pending_delete(&name);
                            self.focused = FocusedArea::ConfirmDelete;
                        }
                    }
                    KeyCode::Enter
                        if self.focused == FocusedArea::Profiles
                            && !self.profiles_state.search.active
                            && key_event.modifiers.contains(KeyModifiers::SHIFT) =>
                    {
                        if let Some(instance) = self.profiles_state.selected_instance() {
                            let dir = self
                                .instance_manager
                                .instances_dir
                                .join(&instance.name)
                                .join(".minecraft");
                            if let Err(e) = std::process::Command::new("xdg-open")
                                .arg(&dir)
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .spawn()
                            {
                                tracing::error!("Failed to open instance directory: {}", e);
                            }
                        }
                    }
                    KeyCode::Enter
                        if self.focused == FocusedArea::Profiles
                            && !self.profiles_state.search.active =>
                    {
                        self.focused = FocusedArea::Content;
                    }
                    KeyCode::Char('l')
                        if self.focused == FocusedArea::Profiles
                            && !self.profiles_state.search.active =>
                    {
                        if let Some(instance) = self.profiles_state.selected_instance().cloned() {
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
                        if self.focused == FocusedArea::Profiles
                            && !self.profiles_state.search.active =>
                    {
                        if let Some(inst) = self.profiles_state.selected_instance() {
                            self.profiles_state.renaming = Some(inst.name.clone());
                        }
                    }
                    KeyCode::Esc
                        if self.focused == FocusedArea::Profiles
                            && !self.profiles_state.search.active =>
                    {
                        if let Some(instance) = self.profiles_state.selected_instance() {
                            crate::running::send_kill(&instance.name);
                        }
                    }
                    _ => {}
                }

                if self.focused == FocusedArea::Profiles {
                    self.profiles_state.handle_key(&key_event)
                }
            }
        }

        if self.profiles_state.wants_popup() {
            self.focused = FocusedArea::Popup;
        } else if self.focused == FocusedArea::Popup {
            self.focused = FocusedArea::Profiles;
        }

        Ok(())
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
                Ok(config) => match pending_instances.lock() {
                    Ok(mut pending) => {
                        pending.push(config);
                    }
                    Err(e) => {
                        tracing::error!("Pending instance queue lock poisoned: {}", e);
                    }
                },
                Err(_e) => {
                    progress::clear();
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

        let editor = std::env::var("EDITOR")
            .or_else(|_| std::env::var("VISUAL"))
            .unwrap_or_else(|_| "vi".to_string());

        let is_tui_editor = matches!(
            editor.rsplit('/').next().unwrap_or(&editor),
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
        } else {
            if let Err(e) = std::process::Command::new(&editor)
                .arg(path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                tracing::error!("Failed to open editor: {}", e);
            }
        }
    }

    fn spawn_launch(&self, instance: crate::instance::InstanceConfig) {
        use crate::instance::launch;
        use crate::running;

        running::set_state(&instance.name, running::RunState::Authenticating);

        let instances_dir = self.instance_manager.instances_dir.clone();
        let meta_dir = self.instance_manager.meta_dir.clone();

        tokio::spawn(async move {
            match launch::launch(&instance, &instances_dir, &meta_dir).await {
                Ok(()) => {}
                Err(e) => {
                    tracing::error!("Failed to launch '{}': {}", instance.name, e);
                    running::remove(&instance.name);
                }
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
                    error_buffer::pop_error();
                }
                _ => break,
            }
        }
    }

    fn drain_pending_instances(&mut self) {
        match PENDING_INSTANCES.lock() {
            Ok(mut pending) => {
                for config in pending.drain(..) {
                    self.profiles_state.add_instance(config);
                }
            }
            Err(e) => {
                tracing::error!("Pending instance queue lock poisoned: {}", e);
            }
        }
    }

    fn drain_pending_last_played(&mut self) {
        for (name, time) in crate::running::drain_last_played() {
            for inst in &mut self.profiles_state.instances {
                if inst.name == name {
                    inst.last_played = Some(time);
                    break;
                }
            }
        }
    }

    fn create_screenshot_protocols(&mut self) {
        let pending = self.screenshots_state.take_pending_images();
        for (idx, img) in pending {
            let proto = self.picker.new_resize_protocol(img);
            self.screenshots_state.set_protocol(idx, proto);
        }
    }

    fn render_log_overlay(&mut self, frame: &mut Frame) {
        use crate::tui::logging::get_app_logs;
        use crate::tui::theme::THEME;
        use ratatui::{
            layout::{Alignment, Margin},
            style::{Modifier, Style},
            text::Line,
            widgets::{Block, Clear, Paragraph, Scrollbar, ScrollbarOrientation},
        };

        let area = frame.area();
        let overlay = area.inner(Margin::new(1, 1));

        frame.render_widget(Clear, overlay);

        let all_lines = get_app_logs();
        let filtered: Vec<&String> = all_lines
            .iter()
            .filter(|l| self.log_overlay_search.matches(l))
            .collect();

        let visible_height = overlay.height.saturating_sub(2) as usize;
        let was_at_bottom =
            self.log_overlay_scroll >= self.log_overlay_max_scroll.saturating_sub(1);
        self.log_overlay_max_scroll = filtered.len().saturating_sub(visible_height);
        if was_at_bottom || self.log_overlay_scroll > self.log_overlay_max_scroll {
            self.log_overlay_scroll = self.log_overlay_max_scroll;
        }
        self.log_overlay_scrollbar =
            ratatui::widgets::ScrollbarState::new(self.log_overlay_max_scroll)
                .position(self.log_overlay_scroll);

        let mut block = Block::bordered()
            .title_top(
                Line::from(" Logs ").style(
                    Style::default()
                        .fg(THEME.log_overlay.text_fg)
                        .add_modifier(Modifier::BOLD),
                ),
            )
            .title_bottom(
                crate::tui::widgets::popups::keybind_line(&[("O", " close"), ("/", " search")])
                    .alignment(Alignment::Right),
            )
            .border_type(THEME.general.border_type.to_border_type())
            .border_style(Style::default().fg(THEME.log_overlay.border_fg));

        if let Some(sl) = self.log_overlay_search.title_line() {
            block = block.title_top(sl);
        }

        let inner = block.inner(overlay);
        frame.render_widget(block, overlay);

        let search = &self.log_overlay_search;
        let styled: Vec<Line> = filtered
            .iter()
            .skip(self.log_overlay_scroll)
            .take(visible_height)
            .map(|line| {
                let style = if line.contains("ERROR") || line.contains("FATAL") {
                    Style::default().fg(THEME.log_overlay.error_fg)
                } else if line.contains("WARN") {
                    Style::default().fg(THEME.log_overlay.warn_fg)
                } else if line.contains("DEBUG") || line.contains("TRACE") {
                    Style::default().fg(THEME.log_overlay.debug_fg)
                } else {
                    Style::default().fg(THEME.log_overlay.text_fg)
                };
                search.highlight_line(line, style)
            })
            .collect();

        frame.render_widget(Paragraph::new(styled), inner);

        let scrollbar_area = ratatui::layout::Rect {
            x: inner.x + inner.width.saturating_sub(1),
            y: inner.y + 1,
            width: 1,
            height: inner.height.saturating_sub(2),
        };
        frame.render_stateful_widget(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight)
                .begin_symbol(Some("\u{25b2}"))
                .style(
                    Style::default()
                        .fg(THEME.log_overlay.border_fg)
                        .add_modifier(Modifier::BOLD),
                )
                .thumb_symbol("\u{2503}")
                .track_symbol(Some(""))
                .end_symbol(Some("\u{25bc}")),
            scrollbar_area,
            &mut self.log_overlay_scrollbar,
        );
    }

    fn sync_error_effects(&mut self, events: &[error_buffer::ErrorEvent]) {
        use crate::tui::theme::THEME;
        let active_ids: std::collections::HashSet<u64> =
            events.iter().map(|event| event.id).collect();
        self.error_effects.retain(|id, _| active_ids.contains(id));

        for event in events {
            self.error_effects.entry(event.id).or_insert_with(|| {
                ErrorEffectState::SlidingIn(fx::slide_in(
                    Motion::RightToLeft,
                    8,
                    0,
                    THEME.log_overlay.bg,
                    (300, Interpolation::SineOut),
                ))
            });
        }
    }

    fn render_error_effect(
        &mut self,
        frame: &mut Frame,
        area: ratatui::layout::Rect,
        event: &error_buffer::ErrorEvent,
        elapsed_ms: u128,
    ) {
        use crate::config::SETTINGS;
        use crate::tui::theme::THEME;
        let fly_out_ms = SETTINGS.ui.error_fly_out_ms as u128;
        let fly_start_ms = SETTINGS.ui.error_auto_dismiss_ms as u128
            - fly_out_ms.min(SETTINGS.ui.error_auto_dismiss_ms as u128);

        if elapsed_ms >= fly_start_ms {
            let entry = self
                .error_effects
                .entry(event.id)
                .or_insert(ErrorEffectState::Idle);
            if !matches!(entry, ErrorEffectState::FadingOut(_)) {
                *entry = ErrorEffectState::FadingOut(fx::slide_out(
                    Motion::LeftToRight,
                    8,
                    0,
                    THEME.log_overlay.bg,
                    (fly_out_ms as u32, Interpolation::SineIn),
                ));
            }
        }

        if let Some(effect_state) = self.error_effects.get_mut(&event.id) {
            match effect_state {
                ErrorEffectState::SlidingIn(effect) => {
                    if effect.running() {
                        frame.render_effect(effect, area, tachyonfx::Duration::from_millis(16));
                    } else {
                        *effect_state = ErrorEffectState::Idle;
                    }
                }
                ErrorEffectState::Idle => {}
                ErrorEffectState::FadingOut(effect) => {
                    if effect.running() {
                        frame.render_effect(effect, area, tachyonfx::Duration::from_millis(16));
                    }
                }
            }
        }
    }
}
