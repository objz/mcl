// crate root. main.rs is a thin wrapper that imports the two entry points
// re-exported below; everything else stays crate-private. integration tests
// in tests/ that need to reach in deeper can use `rmcl::auth`, `rmcl::net`,
// etc. directly; cli + migrate stay private because they have nothing
// general to expose.

pub mod auth;
mod cli;
pub mod config;
pub mod feedback;
pub mod instance;
pub mod launch_profile;
pub mod layout_migration;
mod migrate;
pub mod net;
pub mod storage;
pub mod tui;

#[cfg(test)]
pub(crate) mod tests;

pub use cli::init as cli_init;
pub use instance::content::provider as content_provider;
pub use instance::logs::live as instance_logs;
pub use instance::runtime as running;
pub use migrate::run_legacy_rename as migrate_legacy_rename;
