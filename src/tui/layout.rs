use super::widgets::popups::new_instance;
use super::{
    widgets::{self, profiles, WidgetKey},
    Tui,
};
use crate::instance::{InstanceConfig, InstanceManager};
use crate::tui::error_buffer;
use crate::tui::progress;
use crate::tui::widgets::popups::confirm::{confirm_popup_area, ConfirmPopup};
use crate::tui::widgets::popups::confirm as confirm_popup;
use crate::tui::widgets::popups::error::{popup_area, ErrorPopup};
use color_eyre::eyre::Context;
use crossterm::event::{self, Event};
use once_cell::sync::Lazy;
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyEventKind},
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
    instance_manager: InstanceManager,
    log_list_state: tui_logger::TuiWidgetState,
    throbber_state: throbber_widgets_tui::ThrobberState,
    throbber_tick: u8,
    error_effects: HashMap<u64, ErrorEffectState>,
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
    Details,
    Status,
    StatusExpanded,
    Popup,
    ErrorPopup,
    ConfirmDelete,
}

impl Default for App {
    fn default() -> Self {
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
            instance_manager: manager,
            log_list_state: tui_logger::TuiWidgetState::new()
                .set_default_display_level(log::LevelFilter::Debug),
            throbber_state: throbber_widgets_tui::ThrobberState::default(),
            throbber_tick: 0,
            error_effects: HashMap::new(),
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
            self.throbber_tick = self.throbber_tick.wrapping_add(1);
            if self.throbber_tick % 8 == 0 {
                self.throbber_state.calc_next();
            }
            tui_logger::move_events();
            terminal.draw(|frame| self.render_frame(frame))?;
            self.handle_events().wrap_err("handle events failed")?;
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
        );

        let bottom_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Percentage(40),
                Constraint::Percentage(30),
            ])
            .split(main_chunks[2]);

        widgets::account::render(frame, bottom_chunks[0], self.focused);
        widgets::details::render(frame, bottom_chunks[1], self.focused);
        widgets::status::render(frame, bottom_chunks[2], self.focused, &mut self.throbber_state);

        if self.focused == FocusedArea::StatusExpanded {
            Self::render_log_overlay(frame, &mut self.log_list_state);
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
            Ok(true) => {
                match event::read() {
                    Ok(Event::Key(key_event)) if key_event.kind == KeyEventKind::Press => self
                        .handle_key_event(key_event)
                        .wrap_err_with(|| format!("handling key event failed:\n{key_event:#?}")),
                    Ok(_) => Ok(()),
                    Err(e) => {
                        tracing::error!("Event read error: {}", e);
                        Ok(())
                    }
                }
            }
            Ok(false) => Ok(()),
            Err(e) => {
                tracing::error!("Event poll error: {}", e);
                Ok(())
            }
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        if self.focused == FocusedArea::StatusExpanded {
            match key_event.code {
                KeyCode::Char('S') | KeyCode::Esc => {
                    self.focused = self.pre_overlay_focused;
                    self.log_list_state.transition(tui_logger::TuiWidgetEvent::EscapeKey);
                    return Ok(());
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.log_list_state
                        .transition(tui_logger::TuiWidgetEvent::DownKey);
                    return Ok(());
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.log_list_state
                        .transition(tui_logger::TuiWidgetEvent::UpKey);
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

        match self.focused {
            FocusedArea::Popup => {
                new_instance::handle_key(&key_event, &mut self.profiles_state);
            }
            _ => {
                if self.focused == FocusedArea::Profiles && self.profiles_state.search.active {
                    self.profiles_state.handle_key(&key_event);
                    return Ok(());
                }

                match key_event.code {
                    KeyCode::Char('q') => self.exit = true,
                    KeyCode::Char('P') => self.focused = FocusedArea::Profiles,
                    KeyCode::Char('C') => self.focused = FocusedArea::Content,
                    KeyCode::Char('A') => self.focused = FocusedArea::Account,
                    KeyCode::Char('D') => self.focused = FocusedArea::Details,
                    KeyCode::Char('S') => {
                        self.pre_overlay_focused = self.focused;
                        self.focused = FocusedArea::StatusExpanded;
                    }
                    KeyCode::Tab if self.focused == FocusedArea::Content => {
                        self.content_tab = self.content_tab.next();
                    }
                    KeyCode::BackTab if self.focused == FocusedArea::Content => {
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

    fn spawn_launch(&self, instance: crate::instance::InstanceConfig) {
        use crate::instance::launch;
        use crate::running;
        use crate::tui::error_buffer;

        let instances_dir = self.instance_manager.instances_dir.clone();
        let meta_dir = self.instance_manager.meta_dir.clone();

        tokio::spawn(async move {
            match launch::launch(&instance, &instances_dir, &meta_dir).await {
                Ok(()) => {}
                Err(e) => {
                    tracing::error!("Launch failed for '{}': {}", instance.name, e);
                    running::remove(&instance.name);
                    error_buffer::push_error(error_buffer::ErrorEvent {
                        id: 0,
                        level: tracing::Level::ERROR,
                        message: format!("Failed to launch '{}': {}", instance.name, e),
                        pushed_at: std::time::Instant::now(),
                    });
                }
            }
        });
    }

    fn dismiss_expired_errors(&self) {
        loop {
            match error_buffer::peek_error() {
                Some(event) if event.pushed_at.elapsed().as_millis() >= error_buffer::AUTO_DISMISS_MS => {
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

    fn render_log_overlay(frame: &mut Frame, log_state: &mut tui_logger::TuiWidgetState) {
        use crate::tui::theme::THEME;
        use ratatui::{
            layout::{Alignment, Margin},
            style::{Modifier, Style},
            text::Line,
            widgets::{Block, BorderType, Clear},
        };
        use tui_logger::TuiLoggerWidget;

        let area = frame.area();
        let overlay = area.inner(Margin::new(1, 1));

        frame.render_widget(Clear, overlay);

        let block = Block::bordered()
            .title_top(Line::from(" Logs ").style(
                Style::default()
                    .fg(THEME.colors.foreground)
                    .add_modifier(Modifier::BOLD),
            ))
            .title_bottom(
                crate::tui::widgets::popups::keybind_line(&[("S", " close")])
                    .alignment(Alignment::Right),
            )
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(THEME.colors.border_focused));

        let widget = TuiLoggerWidget::default()
            .block(block)
            .state(log_state)
            .style_error(Style::default().fg(THEME.colors.error))
            .style_warn(Style::default().fg(THEME.colors.warn))
            .style_info(Style::default().fg(THEME.colors.foreground))
            .style_debug(Style::default().fg(THEME.colors.text_idle));

        frame.render_widget(widget, overlay);
    }

    fn sync_error_effects(&mut self, events: &[error_buffer::ErrorEvent]) {
        use crate::tui::theme::THEME;
        let active_ids: std::collections::HashSet<u64> = events.iter().map(|event| event.id).collect();
        self.error_effects.retain(|id, _| active_ids.contains(id));

        for event in events {
            self.error_effects.entry(event.id).or_insert_with(|| {
                ErrorEffectState::SlidingIn(fx::slide_in(
                    Motion::RightToLeft,
                    8,
                    0,
                    THEME.colors.popup_bg,
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
        use crate::tui::theme::THEME;
        const FLY_OUT_MS: u128 = 300;
        let fly_start_ms = error_buffer::AUTO_DISMISS_MS.saturating_sub(FLY_OUT_MS);

        if elapsed_ms >= fly_start_ms {
            let entry = self.error_effects.entry(event.id).or_insert(ErrorEffectState::Idle);
            if !matches!(entry, ErrorEffectState::FadingOut(_)) {
                *entry = ErrorEffectState::FadingOut(fx::slide_out(
                    Motion::LeftToRight,
                    8,
                    0,
                    THEME.colors.popup_bg,
                    (FLY_OUT_MS as u32, Interpolation::SineIn),
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
