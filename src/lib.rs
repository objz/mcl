//! rmcl crate root. main.rs is a thin wrapper around this library so
//! integration tests in tests/ can import everything.

pub mod auth;
pub mod cli;
pub mod config;
pub mod instance;
pub mod instance_logs;
pub mod launch_profile;
pub mod migrate;
pub mod net;
pub mod running;
pub mod tui;
