use super::widgets::popups::new_instance;
use super::{
    widgets::{self, profiles, WidgetKey},
    Tui,
};
use color_eyre::eyre::Context;
use crossterm::event::{self, Event};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, KeyEventKind},
    layout::{Constraint, Direction, Layout},
    Frame,
};

#[derive(Debug, Default)]
pub struct App {
    exit: bool,
    focused: FocusedArea,
    profiles_state: profiles::State,
}

#[derive(Default, Debug, Clone, Copy, PartialEq)]
pub enum FocusedArea {
    #[default]
    Profiles,
    Content,
    Account,
    Details,
    Status,
    Popup,
}

impl App {
    pub fn run(&mut self, terminal: &mut Tui) -> color_eyre::Result<()> {
        while !self.exit {
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
    }

    fn handle_events(&mut self) -> color_eyre::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => self
                .handle_key_event(key_event)
                .wrap_err_with(|| format!("handling key event failed:\n{key_event:#?}")),
            _ => Ok(()),
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
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
                    KeyCode::Char('S') => self.focused = FocusedArea::Status,
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
}
