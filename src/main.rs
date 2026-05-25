#[tokio::main]
async fn main() {
    // Run before logging::init() so the cache rename isn't blocked by a
    // freshly-created ~/.cache/rmcl/ directory.
    rmcl::migrate::run_legacy_rename();

    // Guard must stay in scope to keep the log file writer alive
    let _guard = rmcl::tui::logging::init();
    if let Err(e) = color_eyre::install() {
        eprintln!("Failed to install color-eyre: {}", e);
    }

    rmcl::cli::init().await
}
