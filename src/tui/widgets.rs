use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn render_instances(frame: &mut Frame, area: Rect) {
    let sidebar = Block::default()
        .title(" Instances ")
        .borders(Borders::ALL);
    frame.render_widget(sidebar, area);
}

pub fn render_title(frame: &mut Frame, area: Rect) {
    let top_bar = Paragraph::new(" TEST INSTANCE NAME").block(Block::default().borders(Borders::ALL)).centered();
    frame.render_widget(top_bar, area);
}

pub fn render_content(frame: &mut Frame, area: Rect) {
    let file_list = Paragraph::new(" TEST CONTENT LIBRARY PAGE").block(Block::default().borders(Borders::ALL));
    frame.render_widget(file_list, area);
}

pub fn render_account(frame: &mut Frame, area: Rect) {
    let metadata = Paragraph::new(" ")
        .block(Block::default().borders(Borders::ALL).title(" Account "));
    frame.render_widget(metadata, area);
}

pub fn render_info(frame: &mut Frame, area: Rect) {
    let clipboard = Paragraph::new(" ")
        .block(Block::default().borders(Borders::ALL).title(" Info "));
    frame.render_widget(clipboard, area);
}

pub fn render_status(frame: &mut Frame, area: Rect) {
    let processes = Paragraph::new(" ")
        .block(Block::default().borders(Borders::ALL).title(" Status "));
    frame.render_widget(processes, area);
}

