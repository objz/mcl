pub mod layout;
pub mod logging;
pub mod error_buffer;
pub mod progress;
pub mod theme;
pub mod widgets;

pub type Tui = ratatui::DefaultTerminal;

pub async fn show() -> color_eyre::Result<()> {
    let mut terminal = ratatui::init();
    let result = layout::App::default().run(&mut terminal).await;
    ratatui::restore();
    result
}
