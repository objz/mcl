use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, Widget},
};

pub struct Popup<'a> {
    pub title: Line<'a>,
    pub border_style: Style,
    pub title_style: Style,
    pub style: Style,
    pub content: Box<dyn Fn(Rect, &mut Buffer) + 'a>,
}

impl<'a> Widget for Popup<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);

        let block = Block::new()
            .title(self.title)
            .title_style(self.title_style)
            .borders(Borders::ALL)
            .border_style(self.border_style);

        let inner_area = block.inner(area);
        block.render(area, buf);

        (self.content)(inner_area, buf);
    }
}
