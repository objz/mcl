// base frame that all popups render inside. handles the border, title bar,
// keybind footer, and optional search indicator. content is injected via closure
// so each popup type only worries about its inner area.

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Clear, Widget},
};

use crate::config::theme::BORDER_STYLE;

type ContentFn<'a> = Box<dyn Fn(Rect, &mut Buffer) + 'a>;

pub struct PopupFrame<'a> {
    pub title: Line<'a>,
    pub border_color: Color,
    pub bg: Option<Color>,
    pub keybinds: Option<Line<'a>>,
    pub search_line: Option<Line<'a>>,
    pub content: ContentFn<'a>,
}

impl<'a> Widget for PopupFrame<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // clear first so the popup doesn't layer on top of whatever was underneath
        Clear.render(area, buf);

        if let Some(bg) = self.bg {
            buf.set_style(area, Style::default().bg(bg));
        }

        let mut block = Block::bordered()
            .title_top(self.title)
            .border_type(BORDER_STYLE.to_border_type())
            .border_style(Style::default().fg(self.border_color));

        if let Some(sl) = self.search_line {
            block = block.title_top(sl.alignment(Alignment::Right));
        }

        if let Some(kb) = self.keybinds {
            block = block.title_bottom(kb.alignment(Alignment::Right));
        }

        let inner = block.inner(area);
        block.render(area, buf);
        (self.content)(inner, buf);
    }
}
