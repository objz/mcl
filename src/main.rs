pub mod auth;
mod cli;
pub mod config;
pub mod instance;
pub mod instance_logs;
pub mod launch_profile;
mod migrate;
pub mod net;
pub mod running;
pub mod tui;

#[tokio::main]
async fn main() {
    // Run before logging::init() so the cache rename isn't blocked by a
    // freshly-created ~/.cache/rmcl/ directory.
    migrate::run_legacy_rename();

    // Guard must stay in scope to keep the log file writer alive
    let _guard = tui::logging::init();
    if let Err(e) = color_eyre::install() {
        eprintln!("Failed to install color-eyre: {}", e);
    }

    cli::init().await
}
