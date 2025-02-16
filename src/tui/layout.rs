use super::{widgets::{self, instances, WidgetKey}, Tui};
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
    instances_state: instances::State,
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
    fn render_frame(&mut self, frame: &mut Frame) {
        // Divide the screen into horizontal chunks
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20), // Instances
                Constraint::Percentage(80), // Main content
                Constraint::Min(1),
            ])
            .split(frame.area());


        // Render Instances
        widgets::instances::render(frame, chunks[0], self.focused, &mut self.instances_state);


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
        widgets::content::title(frame, main_chunks[0], self.focused);
        widgets::content::render(frame, main_chunks[1], self.focused);

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
        widgets::account::render(frame, bottom_chunks[0], self.focused);
        widgets::details::render(frame, bottom_chunks[1], self.focused);
        widgets::status::render(frame, bottom_chunks[2], self.focused);
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
            KeyCode::Char('q') => self.exit = true,
            KeyCode::Char('I') => self.focused = FocusedArea::Instances,
            KeyCode::Char('C') => self.focused = FocusedArea::Content,
            KeyCode::Char('A') => self.focused = FocusedArea::Account,
            KeyCode::Char('D') => self.focused = FocusedArea::Details,
            KeyCode::Char('S') => self.focused = FocusedArea::Status,
            _ => {}
        }

        match self.focused {
            FocusedArea::Instances => self.instances_state.handle_key(&key_event),
            _ => {}
        }
        Ok(())
    }

}
