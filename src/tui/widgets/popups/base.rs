use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Clear, Widget},
};

pub struct PopupFrame<'a> {
    pub title: Line<'a>,
    pub border_color: Color,
    pub bg: Option<Color>,
    pub keybinds: Option<Line<'a>>,
    pub content: Box<dyn Fn(Rect, &mut Buffer) + 'a>,
}

impl<'a> Widget for PopupFrame<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        if let Some(bg) = self.bg {
            buf.set_style(area, Style::default().bg(bg));
        }

        let mut block = Block::default()
            .title_top(self.title)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(self.border_color));

        if let Some(kb) = self.keybinds {
            block = block.title_bottom(kb.alignment(Alignment::Right));
        }

        let inner = block.inner(area);
        block.render(area, buf);
        (self.content)(inner, buf);
    }
}
