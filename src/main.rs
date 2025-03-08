use config::SETTINGS;
use logger::Logger;

mod cli;
pub mod config;
pub mod logger;
pub mod macros;
pub mod tui;

fn main() {
    Logger::init(SETTINGS.general.debug);
    cli::init()
}
