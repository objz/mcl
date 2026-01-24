use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Widget},
    Frame,
};
use tui_widget_list::{ListBuilder, ListState as TuiListState, ListView};

use crate::auth::{self, AccountStore, AccountType, AuthResult, DeviceCodeInfo};
use crate::tui::layout::FocusedArea;
use crate::tui::theme::THEME;

use super::styled_title;

#[derive(Default)]
pub enum AddMode {
    #[default]
    None,
    ChooseType,
    OfflineNameInput(String),
    ConfirmDelete(usize),
    DeviceCodeWaiting {
        info: DeviceCodeInfo,
        pending: Arc<Mutex<Option<AuthResult>>>,
    },
}

pub struct AccountState {
    pub store: AccountStore,
    pub list_state: TuiListState,
    pub add_mode: AddMode,
}

impl Default for AccountState {
    fn default() -> Self {
        let store = AccountStore::load();
        let mut list_state = TuiListState::default();
        if !store.accounts.is_empty() {
            list_state.selected = Some(0);
        }
        Self {
            store,
            list_state,
            add_mode: AddMode::None,
        }
    }
}

impl AccountState {
    pub fn drain_auth_result(&mut self) {
        if let AddMode::DeviceCodeWaiting { pending, .. } = &self.add_mode {
            let result = if let Ok(mut slot) = pending.lock() {
                slot.take()
            } else {
                None
            };

            if let Some(result) = result {
                match result {
                    AuthResult::Success(account) => {
                        self.store.add(account);
                        self.add_mode = AddMode::None;
                        if self.list_state.selected.is_none() && !self.store.accounts.is_empty() {
                            self.list_state.selected = Some(0);
                        }
                    }
                    AuthResult::Error(e) => {
                        tracing::error!("Microsoft auth failed: {}", e);
                        self.add_mode = AddMode::None;
                    }
                }
            }
        }
    }
}

pub fn handle_key(key_event: &KeyEvent, state: &mut AccountState) -> bool {
    match &state.add_mode {
        AddMode::ChooseType => match key_event.code {
            KeyCode::Char('m') | KeyCode::Char('1') => {
                let pending = auth::start_microsoft_auth();
                state.add_mode = AddMode::DeviceCodeWaiting {
                    info: DeviceCodeInfo {
                        user_code: String::new(),
                        verification_uri: String::new(),
                    },
                    pending,
                };
                true
            }
            KeyCode::Char('o') | KeyCode::Char('2') => {
                state.add_mode = AddMode::OfflineNameInput(String::new());
                true
            }
            KeyCode::Esc => {
                state.add_mode = AddMode::None;
                true
            }
            _ => true,
        },
        AddMode::OfflineNameInput(name) => match key_event.code {
            KeyCode::Enter => {
                let trimmed = name.trim().to_string();
                if !trimmed.is_empty() {
                    let account = auth::create_offline_account(&trimmed);
                    state.store.add(account);
                    if state.list_state.selected.is_none() && !state.store.accounts.is_empty() {
                        state.list_state.selected = Some(0);
                    }
                }
                state.add_mode = AddMode::None;
                true
            }
            KeyCode::Char(c) => {
                let mut new_name = name.clone();
                new_name.push(c);
                state.add_mode = AddMode::OfflineNameInput(new_name);
                true
            }
            KeyCode::Backspace => {
                let mut new_name = name.clone();
                new_name.pop();
                state.add_mode = AddMode::OfflineNameInput(new_name);
                true
            }
            KeyCode::Esc => {
                state.add_mode = AddMode::None;
                true
            }
            _ => true,
        },
        AddMode::DeviceCodeWaiting { .. } => match key_event.code {
            KeyCode::Esc => {
                state.add_mode = AddMode::None;
                true
            }
            _ => true,
        },
        AddMode::ConfirmDelete(idx) => {
            let idx = *idx;
            match key_event.code {
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let count = state.store.accounts.len();
                    state.store.remove(idx);
                    if count > 1 {
                        state.list_state.selected =
                            Some(idx.min(state.store.accounts.len().saturating_sub(1)));
                    } else {
                        state.list_state.selected = None;
                    }
                    state.add_mode = AddMode::None;
                    true
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                    state.add_mode = AddMode::None;
                    true
                }
                _ => true,
            }
        }
        AddMode::None => {
            let count = state.store.accounts.len();
            match key_event.code {
                KeyCode::Char('a') => {
                    state.add_mode = AddMode::ChooseType;
                    true
                }
                KeyCode::Char('d') => {
                    if let Some(idx) = state.list_state.selected {
                        state.add_mode = AddMode::ConfirmDelete(idx);
                    }
                    true
                }
                KeyCode::Enter => {
                    if let Some(idx) = state.list_state.selected {
                        state.store.set_active(idx);
                    }
                    true
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if count > 0 {
                        let cur = state.list_state.selected.unwrap_or(0);
                        state.list_state.selected = Some((cur + 1).min(count - 1));
                    }
                    true
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    let cur = state.list_state.selected.unwrap_or(0);
                    state.list_state.selected = Some(cur.saturating_sub(1));
                    true
                }
                _ => false,
            }
        }
    }
}

pub fn drain_device_code(state: &mut AccountState) {
    if let AddMode::DeviceCodeWaiting { info, .. } = &mut state.add_mode {
        if info.user_code.is_empty() {
            if let Ok(mut slot) = auth::DEVICE_CODE_DISPLAY.lock() {
                if let Some(dc_info) = slot.take() {
                    info.user_code = dc_info.user_code;
                    info.verification_uri = dc_info.verification_uri;
                }
            }
        }
    }
}

pub fn render(frame: &mut Frame, area: Rect, focused: FocusedArea, state: &mut AccountState) {
    let color = if focused == FocusedArea::Account {
        THEME.colors.border_focused
    } else {
        THEME.colors.border_unfocused
    };

    let mut block = Block::default()
        .title(styled_title("Accounts", true))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color));

    if focused == FocusedArea::Account {
        let lines = super::popups::keybind_lines_wrapped(
            &[("⏎", " select"), ("a", " add"), ("d", " del")],
            area.width.saturating_sub(2),
        );
        for line in lines {
            block = block.title_bottom(line);
        }
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.store.accounts.is_empty() {
        frame.render_widget(
            Paragraph::new("No accounts.").style(Style::default().fg(THEME.colors.text_idle)),
            inner,
        );
    } else {
        let active = state.store.active_account().map(|a| a.uuid.clone());
        render_account_list(frame, inner, state, focused, active.as_deref());
    }

    match &state.add_mode {
        AddMode::ChooseType => render_choose_popup(frame),
        AddMode::OfflineNameInput(name) => render_offline_popup(frame, name),
        AddMode::ConfirmDelete(idx) => render_confirm_delete(frame, state, *idx),
        AddMode::DeviceCodeWaiting { info, .. } => render_device_code_popup(frame, info),
        AddMode::None => {}
    }
}

fn render_account_list(
    frame: &mut Frame,
    area: Rect,
    state: &mut AccountState,
    focused: FocusedArea,
    active_uuid: Option<&str>,
) {
    let is_focused = focused == FocusedArea::Account;
    let accounts: Vec<(String, AccountType, bool)> = state
        .store
        .accounts
        .iter()
        .map(|a| {
            (
                a.username.clone(),
                a.account_type.clone(),
                active_uuid == Some(&a.uuid),
            )
        })
        .collect();

    let count = accounts.len();

    let builder = ListBuilder::new(move |context| {
        let (username, acc_type, is_active) = &accounts[context.index];
        let show_selected = is_focused && context.is_selected;

        let stripe_bg = if context.index % 2 == 0 {
            Color::Reset
        } else {
            THEME.colors.row_alternate_bg
        };

        let bg = if show_selected {
            THEME.colors.row_background
        } else {
            stripe_bg
        };

        let active_marker = if *is_active { "\u{25b8} " } else { "  " };

        let style = if show_selected {
            Style::default()
                .fg(THEME.colors.row_highlight)
                .add_modifier(Modifier::BOLD)
        } else if *is_active {
            Style::default()
                .fg(THEME.colors.foreground)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(THEME.colors.foreground)
        };

        let mut spans = vec![
            Span::styled(active_marker, Style::default().fg(THEME.colors.accent)),
            Span::styled(username.clone(), style),
        ];

        if *acc_type == AccountType::Offline {
            let offline_style = if show_selected {
                Style::default().fg(THEME.colors.row_highlight)
            } else {
                Style::default().fg(THEME.colors.text_idle)
            };
            spans.push(Span::styled(" (Offline)", offline_style));
        }

        let item = ratatui::text::Text::from(Line::from(spans)).style(Style::default().bg(bg));
        (item, 1)
    });

    let list = ListView::new(builder, count);
    frame.render_stateful_widget(list, area, &mut state.list_state);
}

fn popup_area(frame: &Frame, width: u16, height: u16) -> Rect {
    let area = frame.area();
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

fn render_choose_popup(frame: &mut Frame) {
    use super::popups::base::PopupFrame;
    let area = popup_area(frame, 40, 7);

    PopupFrame {
        title: Line::from(" Add Account ").centered(),
        border_color: THEME.colors.border_focused,
        bg: None,
        keybinds: Some(Line::from(Span::styled(
            " Esc: cancel ",
            Style::default().fg(THEME.colors.text_idle),
        ))),
        search_line: None,
        content: Box::new(|inner, buf| {
            let text = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        " [m] ",
                        Style::default()
                            .fg(THEME.colors.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "Microsoft Account",
                        Style::default().fg(THEME.colors.foreground),
                    ),
                ]),
                Line::from(vec![
                    Span::styled(
                        " [o] ",
                        Style::default()
                            .fg(THEME.colors.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        "Offline Account",
                        Style::default().fg(THEME.colors.foreground),
                    ),
                ]),
            ];
            Paragraph::new(text).render(inner, buf);
        }),
    }
    .render(area, frame.buffer_mut());
}

fn render_offline_popup(frame: &mut Frame, name: &str) {
    use super::popups::{base::PopupFrame, keybind_line};
    let area = popup_area(frame, 40, 5);
    let name = name.to_string();

    PopupFrame {
        title: Line::from(Span::styled(
            " Offline Account ",
            Style::default()
                .fg(THEME.colors.border_focused)
                .add_modifier(Modifier::BOLD),
        ))
        .centered(),
        border_color: THEME.colors.border_focused,
        bg: Some(THEME.colors.popup_bg),
        keybinds: Some(keybind_line(&[("Enter", " confirm"), ("Esc", " cancel")])),
        search_line: None,
        content: Box::new(move |inner, buf| {
            let line = if name.is_empty() {
                Line::from(vec![
                    Span::styled(
                        "Username...",
                        Style::default().fg(THEME.colors.border_unfocused),
                    ),
                    Span::styled(
                        "\u{2588}",
                        Style::default()
                            .fg(THEME.colors.border_focused)
                            .add_modifier(Modifier::SLOW_BLINK),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::styled(name.as_str(), Style::default().fg(THEME.colors.foreground)),
                    Span::styled(
                        "\u{2588}",
                        Style::default()
                            .fg(THEME.colors.border_focused)
                            .add_modifier(Modifier::SLOW_BLINK),
                    ),
                ])
            };
            Paragraph::new(line).render(inner, buf);
        }),
    }
    .render(area, frame.buffer_mut());
}

fn render_confirm_delete(frame: &mut Frame, state: &AccountState, idx: usize) {
    use super::popups::{base::PopupFrame, keybind_line};
    let username = state
        .store
        .accounts
        .get(idx)
        .map(|a| a.username.as_str())
        .unwrap_or("?");

    let title = Line::from(Span::styled(
        format!(" Delete '{}' ", username),
        Style::default()
            .fg(THEME.colors.border_focused)
            .add_modifier(Modifier::BOLD),
    ));

    let body = "This will permanently remove this account";
    let popup_w = (username.len() + 14).max(body.len() + 2).min(48) as u16 + 2;
    let area = popup_area(frame, popup_w, 3);

    PopupFrame {
        title,
        border_color: THEME.colors.border_focused,
        bg: Some(THEME.colors.row_alternate_bg),
        keybinds: Some(keybind_line(&[("Enter", " confirm")])),
        search_line: None,
        content: Box::new(|inner, buf| {
            Paragraph::new("This will permanently remove this account")
                .style(Style::default().fg(THEME.colors.foreground))
                .render(inner, buf);
        }),
    }
    .render(area, frame.buffer_mut());
}

fn render_device_code_popup(frame: &mut Frame, info: &DeviceCodeInfo) {
    use super::popups::{base::PopupFrame, keybind_line};
    let area = popup_area(frame, 50, 8);
    let uri = info.verification_uri.clone();
    let code = info.user_code.clone();

    PopupFrame {
        title: Line::from(Span::styled(
            " Microsoft Login ",
            Style::default()
                .fg(THEME.colors.border_focused)
                .add_modifier(Modifier::BOLD),
        ))
        .centered(),
        border_color: THEME.colors.border_focused,
        bg: Some(THEME.colors.popup_bg),
        keybinds: Some(keybind_line(&[("Esc", " cancel")])),
        search_line: None,
        content: Box::new(move |inner, buf| {
            let text = if code.is_empty() {
                vec![Line::from(Span::styled(
                    "Connecting to Microsoft...",
                    Style::default().fg(THEME.colors.text_idle),
                ))]
            } else {
                vec![
                    Line::from(Span::styled(
                        "Open this URL in your browser:",
                        Style::default().fg(THEME.colors.text_idle),
                    )),
                    Line::from(Span::styled(
                        uri.as_str(),
                        Style::default()
                            .fg(THEME.colors.accent)
                            .add_modifier(Modifier::BOLD),
                    )),
                    Line::from(""),
                    Line::from(vec![
                        Span::styled("Enter code: ", Style::default().fg(THEME.colors.text_idle)),
                        Span::styled(
                            code.as_str(),
                            Style::default()
                                .fg(THEME.colors.success)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]),
                    Line::from(""),
                    Line::from(Span::styled(
                        "Waiting for authentication...",
                        Style::default().fg(THEME.colors.text_idle),
                    )),
                ]
            };
            Paragraph::new(text).render(inner, buf);
        }),
    }
    .render(area, frame.buffer_mut());
}
