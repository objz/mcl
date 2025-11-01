mod cli;
pub mod config;
pub mod instance;
pub mod net;
pub mod instance_logs;
pub mod running;
pub mod tui;

#[tokio::main]
async fn main() {
    let _guard = tui::logging::init();
    if let Err(e) = color_eyre::install() {
        eprintln!("Failed to install color-eyre: {}", e);
    }
    cli::init().await
}
