use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span}, 
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use super::layout::FocusedArea;

fn styled_title(title: &str, highlight: bool) -> Line {
    if !highlight || title.is_empty() {
        Line::from(Span::raw(title))
    } else {
        let mut chars = title.chars();
        let first = chars.next().unwrap_or_default().to_string();
        let rest: String = chars.collect();
        Line::from(vec![
            Span::styled(first, Style::default().fg(Color::Yellow)),
            Span::raw(rest),
        ])
    }
}

fn render_section(
    frame: &mut Frame,
    area: Rect,
    focused: FocusedArea,
    title: &str,
    content: &str,
    focus_area: FocusedArea,
    highlight: bool,
) {
    let color = if focused == focus_area {
        Color::White
    } else {
        Color::DarkGray
    };

    let block = Block::default()
        .title(styled_title(title, highlight))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color));

    let widget = Paragraph::new(content).block(block);
    frame.render_widget(widget, area);
}

pub fn render_instances(frame: &mut Frame, area: Rect, focused: FocusedArea) {
    render_section(
        frame,
        area,
        focused,
        "Instances",
        "",
        FocusedArea::Instances,
        true,
    );
}

pub fn render_title(frame: &mut Frame, area: Rect, focused: FocusedArea) {
    render_section(
        frame,
        area,
        focused,
        "Title",
        " TEST INSTANCE NAME",
        FocusedArea::Content,
        false,
    );
}

pub fn render_content(frame: &mut Frame, area: Rect, focused: FocusedArea) {
    render_section(
        frame,
        area,
        focused,
        "Content",
        " TEST CONTENT LIBRARY PAGE",
        FocusedArea::Content,
        true,
    );
}

pub fn render_account(frame: &mut Frame, area: Rect, focused: FocusedArea) {
    render_section(
        frame,
        area,
        focused,
        "Account",
        "",
        FocusedArea::Account,
        true,
    );
}

pub fn render_details(frame: &mut Frame, area: Rect, focused: FocusedArea) {
    render_section(
        frame,
        area,
        focused,
        "Details",
        "",
        FocusedArea::Details,
        true,
    );
}

pub fn render_status(frame: &mut Frame, area: Rect, focused: FocusedArea) {
    render_section(
        frame,
        area,
        focused,
        "Status",
        "",
        FocusedArea::Status,
        true,
    );
}
