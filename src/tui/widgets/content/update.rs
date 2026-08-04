use std::sync::{Arc, Mutex};

use ratatui::{
    Frame,
    layout::{Constraint, Flex, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::config::theme::{BORDER_STYLE, THEME};
use crate::instance::ContentKind;
use crate::instance::content::updates::{BulkUpdatePlan, UpdateSnapshot};

pub enum PendingResult {
    Prepared(UpdateSnapshot, BulkUpdatePlan),
    Applied(Result<(), String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Checking,
    Conflicts,
    Review,
    Applying,
}

pub struct State {
    pub phase: Phase,
    pub plan: Option<BulkUpdatePlan>,
    pub snapshot: Option<UpdateSnapshot>,
    pub selected: usize,
    pub retry_conflicts: Vec<bool>,
    pub error: Option<String>,
    pub pending: Arc<Mutex<Vec<PendingResult>>>,
    pub kind: ContentKind,
    pub target_world: Option<(String, std::path::PathBuf)>,
    pub completed: bool,
}

impl State {
    pub fn checking(kind: ContentKind, target_world: Option<(String, std::path::PathBuf)>) -> Self {
        Self {
            phase: Phase::Checking,
            plan: None,
            snapshot: None,
            selected: 0,
            retry_conflicts: Vec::new(),
            error: None,
            pending: Arc::new(Mutex::new(Vec::new())),
            kind,
            target_world,
            completed: false,
        }
    }

    pub fn drain(&mut self) -> bool {
        let pending = match self.pending.lock() {
            Ok(mut pending) => pending.drain(..).collect::<Vec<_>>(),
            Err(_) => return false,
        };
        let mut changed = false;
        for result in pending {
            changed = true;
            match result {
                PendingResult::Prepared(snapshot, plan) => {
                    self.retry_conflicts = vec![false; plan.conflicts.len()];
                    self.phase = if plan.conflicts.is_empty() {
                        Phase::Review
                    } else {
                        Phase::Conflicts
                    };
                    self.selected = 0;
                    self.snapshot = Some(snapshot);
                    self.plan = Some(plan);
                    self.error = None;
                }
                PendingResult::Applied(Ok(())) => self.completed = true,
                PendingResult::Applied(Err(error)) => {
                    self.phase = Phase::Review;
                    self.error = Some(error);
                }
            }
        }
        changed
    }

    pub fn push(&self, result: PendingResult) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(result);
            crate::feedback::request_redraw();
        }
    }

    pub fn has_retry(&self) -> bool {
        self.retry_conflicts.iter().any(|retry| *retry)
    }
}

pub fn render(frame: &mut Frame, state: &State) {
    let theme = THEME.as_ref();
    let item_count = match state.phase {
        Phase::Conflicts => state.plan.as_ref().map_or(0, |plan| plan.conflicts.len()),
        Phase::Review => state.plan.as_ref().map_or(0, |plan| plan.roots.len()),
        _ => 1,
    };
    let height = (item_count as u16 + 5).clamp(7, 20);
    let [area] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(frame.area());
    let [area] = Layout::horizontal([Constraint::Percentage(62)])
        .flex(Flex::Center)
        .areas(area);
    frame.render_widget(Clear, area);

    let (title, footer) = match state.phase {
        Phase::Checking => (" Check for updates ", " [Esc] close "),
        Phase::Conflicts => (
            " Resolve update conflicts ",
            " [j/k] select  [Space] keep/retry  [Enter] continue  [Esc] close ",
        ),
        Phase::Review => (
            " Update installed content ",
            " [Enter] update all  [Esc] close ",
        ),
        Phase::Applying => (" Updating installed content ", " Please wait "),
    };
    let block = Block::default()
        .title(title)
        .title_bottom(Line::from(footer).centered())
        .borders(Borders::ALL)
        .border_type(BORDER_STYLE.to_border_type())
        .border_style(Style::default().fg(theme.accent()))
        .style(Style::default().fg(theme.text()).bg(theme.surface()));

    match state.phase {
        Phase::Checking | Phase::Applying => {
            let message = if state.phase == Phase::Checking {
                "Checking compatible versions and dependencies…"
            } else {
                "Installing the accepted update set…"
            };
            frame.render_widget(Paragraph::new(message).block(block), area);
        }
        Phase::Conflicts => {
            let items = state
                .plan
                .as_ref()
                .into_iter()
                .flat_map(|plan| plan.conflicts.iter())
                .enumerate()
                .map(|(index, conflict)| {
                    let action = if state.retry_conflicts.get(index) == Some(&true) {
                        "Retry"
                    } else {
                        "Keep"
                    };
                    ListItem::new(vec![
                        Line::from(vec![
                            Span::styled(
                                conflict.title.clone(),
                                Style::default().add_modifier(Modifier::BOLD),
                            ),
                            Span::raw("  "),
                            Span::styled(action, Style::default().fg(theme.accent())),
                        ]),
                        Line::from(conflict.reason.clone())
                            .style(Style::default().fg(theme.text_dim())),
                    ])
                });
            let mut selected = ListState::default().with_selected(Some(state.selected));
            let list = List::new(items)
                .block(block)
                .highlight_symbol("▌")
                .highlight_style(Style::default().bg(theme.stripe()));
            frame.render_stateful_widget(list, area, &mut selected);
        }
        Phase::Review => {
            let mut items = state
                .plan
                .as_ref()
                .into_iter()
                .flat_map(|plan| plan.roots.iter())
                .map(|root| {
                    ListItem::new(Line::from(format!(
                        "• {}  {} → {}",
                        root.title, root.current_version, root.target.version_number
                    )))
                })
                .collect::<Vec<_>>();
            if items.is_empty() {
                items.push(ListItem::new("No compatible updates available."));
            }
            if let Some(error) = &state.error {
                items.push(ListItem::new(
                    Line::from(error.clone()).style(Style::default().fg(theme.error())),
                ));
            }
            frame.render_widget(List::new(items).block(block), area);
        }
    }
}
