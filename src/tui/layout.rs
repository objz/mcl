use super::widgets;
use super::Tui;
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
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FocusedArea {
    Instances,
    Content,
    Account,
    Details,
    Status,
}

impl Default for FocusedArea {
    fn default() -> Self {
        FocusedArea::Instances
    }
}

impl App {
    /// runs the main loop until the user quits
    pub fn run(&mut self, terminal: &mut Tui) -> color_eyre::Result<()> {
        while !self.exit {
            terminal.draw(|frame| self.render_frame(frame))?;
            self.handle_events().wrap_err("handle events failed")?;
        }
        Ok(())
    }
    fn render_frame(&self, frame: &mut Frame) {
        // Divide the screen into horizontal chunks
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20), // Instances
                Constraint::Percentage(80), // Main content
            ])
            .split(frame.area());

        // Render Instances
        widgets::render_instances(frame, chunks[0], self.focused);

        // Divide the main content into vertical chunks
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Instances
                Constraint::Min(10),   // Main content
                Constraint::Length(5), // Bottom panel
            ])
            .split(chunks[1]);

        // Render widgets in the main content
        widgets::render_title(frame, main_chunks[0], self.focused);
        widgets::render_content(frame, main_chunks[1], self.focused);

        // Bottom panel split
        let bottom_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(30), // Account
                Constraint::Percentage(40), // Info
                Constraint::Percentage(30), // Status
            ])
            .split(main_chunks[2]);

        // Render bottom widgets
        widgets::render_account(frame, bottom_chunks[0], self.focused);
        widgets::render_details(frame, bottom_chunks[1], self.focused);
        widgets::render_status(frame, bottom_chunks[2], self.focused);
    }

    /// updates the applications state based on user input
    fn handle_events(&mut self) -> color_eyre::Result<()> {
        match event::read()? {
            Event::Key(key_event) if key_event.kind == KeyEventKind::Press => self
                .handle_key_event(key_event)
                .wrap_err_with(|| format!("handling key event failed:\n{key_event:#?}")),
            _ => Ok(()),
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> color_eyre::Result<()> {
        match key_event.code {
            KeyCode::Char('q') => self.exit(),
            KeyCode::Char('I') => self.focused = FocusedArea::Instances,
            KeyCode::Char('C') => self.focused = FocusedArea::Content,
            KeyCode::Char('A') => self.focused = FocusedArea::Account,
            KeyCode::Char('D') => self.focused = FocusedArea::Details,
            KeyCode::Char('S') => self.focused = FocusedArea::Status,
            _ => {}
        }
        Ok(())
    }

    fn exit(&mut self) {
        self.exit = true;
    }
}
