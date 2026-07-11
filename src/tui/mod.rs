// tui entrypoint: sets up the terminal, runs the app, cleans up on exit.

pub mod app;
pub mod error_buffer;
mod event;
mod input;
pub mod logging;
pub mod progress;
mod render;
pub mod widgets;

use std::sync::atomic::{AtomicBool, Ordering};

static REDRAW_REQUESTED: AtomicBool = AtomicBool::new(true);

pub fn request_redraw() {
    REDRAW_REQUESTED.store(true, Ordering::Release);
}

pub(super) fn take_redraw_request() -> bool {
    REDRAW_REQUESTED.swap(false, Ordering::AcqRel)
}

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
    let mut picker = ratatui_image::picker::Picker::from_query_stdio()
        .unwrap_or_else(|_| ratatui_image::picker::Picker::halfblocks());
    let detected_protocol = picker.protocol_type();
    let requested_protocol = match crate::config::SETTINGS.ui.image_protocol {
        crate::config::settings::ImageProtocol::Halfblocks
        | crate::config::settings::ImageProtocol::Quadrants => {
            ratatui_image::picker::ProtocolType::Halfblocks
        }
        crate::config::settings::ImageProtocol::Kitty
            if detected_protocol == ratatui_image::picker::ProtocolType::Kitty =>
        {
            ratatui_image::picker::ProtocolType::Kitty
        }
        crate::config::settings::ImageProtocol::Iterm2
            if detected_protocol == ratatui_image::picker::ProtocolType::Iterm2 =>
        {
            ratatui_image::picker::ProtocolType::Iterm2
        }
        _ => ratatui_image::picker::ProtocolType::Halfblocks,
    };
    picker.set_protocol_type(requested_protocol);

    let result = app::App::new(picker).run(&mut terminal).await;

    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::PopKeyboardEnhancementFlags
    );

    ratatui::restore();
    result
}
