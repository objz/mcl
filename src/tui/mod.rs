pub mod layout;
pub mod log_buffer;
pub mod logging;
pub mod error_buffer;
pub mod progress;
pub mod theme;
pub mod widgets;

use std::io::{stdout, Result, Stdout};

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{prelude::CrosstermBackend, Terminal};

pub async fn show() -> color_eyre::Result<()> {
    let mut terminal = init_ratatui()?;
    let result = layout::App::default().run(&mut terminal).await;
    if let Err(err) = restore_ratatui() {
        eprintln!(
            "failed to restore terminal. Run 'reset' or restart your terminal to recover: {}",
            err
        );
    }
    result
}

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

fn init_ratatui() -> Result<Tui> {
    execute!(stdout(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    set_panic_hook();
    Terminal::new(CrosstermBackend::new(stdout()))
}

fn set_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_ratatui();
        hook(panic_info);
    }));
}

fn restore_ratatui() -> Result<()> {
    execute!(stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}
