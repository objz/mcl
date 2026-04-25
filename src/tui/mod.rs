// tui entrypoint: sets up the terminal, runs the app, cleans up on exit.

pub mod app;
pub mod error_buffer;
mod event;
mod input;
pub mod logging;
pub mod progress;
mod render;
pub mod widgets;

pub type Tui = ratatui::DefaultTerminal;

pub async fn show() -> color_eyre::Result<()> {
    // restore the terminal before printing a panic. without this, a panic
    // leaves raw mode + alternate screen active and looks like a freeze
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::event::PopKeyboardEnhancementFlags
        );
        ratatui::restore();
        default_hook(info);
    }));

    let mut terminal = ratatui::init();

    // opt into enhanced keyboard protocol to distinguish key press vs release
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::PushKeyboardEnhancementFlags(
            crossterm::event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                | crossterm::event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        )
    );

    // figure out the terminal's font cell size for rendering images.
    // falls back to halfblock characters if the terminal doesn't respond
    let picker = ratatui_image::picker::Picker::from_query_stdio()
        .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks());

    let result = app::App::new(picker).run(&mut terminal).await;

    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::PopKeyboardEnhancementFlags
    );

    ratatui::restore();
    result
}
