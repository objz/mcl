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
use std::sync::{Arc, Mutex};
use std::time::Duration;

static PENDING_INSTANCES: Lazy<Arc<Mutex<Vec<InstanceConfig>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

pub struct App {
    exit: bool,
    focused: FocusedArea,
    pre_overlay_focused: FocusedArea,
    profiles_state: profiles::State,
    instance_manager: InstanceManager,
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

        match std::fs::create_dir_all(&instances_dir) {
            Ok(_) => {}
            Err(e) => {
                tracing::error!("Failed to create instances dir: {}", e);
            }
        }

        let manager = InstanceManager::new(instances_dir);
        let instances = manager.load_all();
        let profiles_state = profiles::State::with_instances(instances);

        App {
            exit: false,
            focused: FocusedArea::default(),
            pre_overlay_focused: FocusedArea::default(),
            profiles_state,
            instance_manager: manager,
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

        widgets::content::title(frame, main_chunks[0], self.focused);
        widgets::content::render(frame, main_chunks[1], self.focused);

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
        widgets::status::render(frame, bottom_chunks[2], self.focused);

        if self.focused == FocusedArea::StatusExpanded {
            Self::render_log_overlay(frame);
        }

        let all_errors = error_buffer::peek_all_errors();
        let mut next_y: u16 = 1;
        for event in all_errors {
            let elapsed_ms = event.pushed_at.elapsed().as_millis();
            match popup_area(frame.area(), &event.message, next_y, elapsed_ms) {
                Some(area) => {
                    next_y = next_y.saturating_add(area.height + 1);
                    frame.render_widget(ErrorPopup::new(event), area);
                }
                None => {}
            }
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
                KeyCode::Char('S') | KeyCode::Char('q') | KeyCode::Esc => {
                    self.focused = self.pre_overlay_focused;
                    return Ok(());
                }
                _ => {
                    return Ok(());
                }
            }
        }

        if self.focused == FocusedArea::ConfirmDelete {
            match key_event.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
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
                KeyCode::Char('n') | KeyCode::Char('N') => {
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
                    KeyCode::Char('d') if self.focused == FocusedArea::Profiles => {
                        if let Some(instance) = self.profiles_state.selected_instance() {
                            let name = instance.name.clone();
                            confirm_popup::set_pending_delete(&name);
                            self.focused = FocusedArea::ConfirmDelete;
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
        let pending_instances = PENDING_INSTANCES.clone();

        tokio::spawn(async move {
            progress::set_action(format!("Creating instance '{}'...", params.name));
            progress::set_sub_action(format!("{} {}", params.game_version, params.loader));

            let manager = InstanceManager::new(instances_dir);
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

    fn render_log_overlay(frame: &mut Frame) {
        use crate::tui::log_buffer;
        use crate::tui::theme::THEME;
        use ratatui::{
            layout::Alignment,
            style::{Color, Modifier, Style},
            text::{Line, Span},
            widgets::{Block, BorderType, Borders, Clear, List, ListItem},
        };

        let area = frame.area();
        let overlay = ratatui::layout::Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };

        frame.render_widget(Clear, overlay);

        let block = Block::default()
            .title(Line::from(vec![Span::styled(
                " Logs ",
                Style::default()
                    .fg(THEME.colors.foreground)
                    .add_modifier(Modifier::BOLD),
            )]))
            .title_bottom(
                crate::tui::widgets::popups::keybind_line(&[("S", " close"), ("Esc", " close")])
                    .alignment(Alignment::Right)
            )
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(THEME.colors.border_focused));

        let inner = block.inner(overlay);
        frame.render_widget(block, overlay);

        let logs = log_buffer::get_logs();
        let items: Vec<ListItem> = logs
            .iter()
            .map(|entry| {
                let (level_str, level_color) = match entry.level {
                    tracing::Level::ERROR => ("ERROR", Color::Red),
                    tracing::Level::WARN => ("WARN ", Color::Yellow),
                    _ => ("INFO ", THEME.colors.border_unfocused),
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} ", level_str),
                        Style::default()
                            .fg(level_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        entry.message.as_str(),
                        Style::default().fg(THEME.colors.foreground),
                    ),
                ]))
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, inner);
    }
}


