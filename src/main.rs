mod cli;
pub mod config;
pub mod tui;

fn main() {
    let _guard = tui::logging::init();
    color_eyre::install().expect("Failed to install color-eyre");
    cli::init()
}
