#[tokio::main]
async fn main() {
    rmcl::migrate_legacy_rename();

    // Guard must stay in scope to keep the log file writer alive
    let _guard = rmcl::tui::logging::init();
    tracing::info!("Starting rmcl {}", env!("CARGO_PKG_VERSION"));
    if let Err(e) = color_eyre::install() {
        eprintln!("Failed to install color-eyre: {}", e);
        tracing::warn!("Failed to install color-eyre handler: {}", e);
    }

    rmcl::cli_init().await
}
