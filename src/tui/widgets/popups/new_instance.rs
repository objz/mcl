use super::base::Popup;
use crate::tui::layout::FocusedArea;
use crate::tui::widgets::profiles;
use crossterm::event::KeyCode;
use ratatui::{
    Frame,
    style::{Color, Style},
    layout::{Rect, Layout, Direction, Constraint},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap, Widget},
};
use once_cell::sync::Lazy;
use std::sync::Mutex;

static INSTANCE_POPUP_STATE: Lazy<Mutex<NewInstanceState>> = Lazy::new(|| Mutex::new(NewInstanceState::default()));

#[derive(Debug, Default)]
enum NewInstanceMode {
    #[default]
    Buttons,
    Input,
}

#[derive(Debug, Default)]
struct NewInstanceState {
    mode: NewInstanceMode,
    input_text: String,
}

pub fn render(frame: &mut Frame, area: Rect, _focused: FocusedArea) {
    let state = INSTANCE_POPUP_STATE.lock().unwrap();

    let popup = Popup {
        title: Line::from("New Instance"),
        content: Box::new(move |area, buf| {
            match state.mode {
                NewInstanceMode::Buttons => {
                    let chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([
                            Constraint::Percentage(40),
                            Constraint::Length(3),
                            Constraint::Length(3),
                            Constraint::Percentage(40),
                        ])
                        .split(area);

                    let create_button = Paragraph::new(Line::from(vec![
                        Span::styled("C", Style::default().fg(Color::Yellow)),
                        Span::raw("reate New Instance"),
                    ])).alignment(ratatui::layout::Alignment::Center);

                    let import_button = Paragraph::new(Line::from(vec![
                        Span::styled("I", Style::default().fg(Color::Yellow)),
                        Span::raw("mport Morinth Modpack"),
                    ])).alignment(ratatui::layout::Alignment::Center);

                    create_button.render(chunks[1], buf);
                    import_button.render(chunks[2], buf);
                }
                NewInstanceMode::Input => {
                    let paragraph = Paragraph::new(state.input_text.clone())
                        .block(Block::default().title("Enter URL or Path").borders(Borders::ALL))
                        .wrap(Wrap { trim: true });

                    paragraph.render(area, buf);
                }
            }
        }),
        border_style: Default::default(),
        title_style: Default::default(),
        style: Default::default(),
    };

    frame.render_widget(popup, area);
}

pub fn handle_key(key_event: &crossterm::event::KeyEvent, state: &mut profiles::State) {
    let mut popup_state = INSTANCE_POPUP_STATE.lock().unwrap();

    match popup_state.mode {
        NewInstanceMode::Buttons => {
            match key_event.code {
                KeyCode::Char('q') => {
                    state.show_popup = false;
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    // Create new instance
                    state.show_popup = false;
                }
                KeyCode::Char('i') | KeyCode::Char('I') => {
                    // Switch to input mode
                    popup_state.mode = NewInstanceMode::Input;
                }
                _ => {}
            }
        }
        NewInstanceMode::Input => match key_event.code {
            KeyCode::Esc => {
                popup_state.mode = NewInstanceMode::Buttons;
            }
            KeyCode::Enter => {
                // Confirm input
                state.show_popup = false;
            }
            KeyCode::Char(c) => {
                popup_state.input_text.push(c);
            }
            KeyCode::Backspace => {
                popup_state.input_text.pop();
            }
            _ => {}
        },
    }
}
