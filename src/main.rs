use std::path::PathBuf;
use crate::config::{ensure_config_exists, load_config};
use logger::Logger;

mod cli;
pub mod logger;
pub mod macros;
pub mod tui;
pub mod config;


fn main() {
   let default = "assets/default.toml";
   let path: PathBuf = ensure_config_exists(default);

   match load_config(&path) {
      Ok(config) => {
         Logger::init(config.variables.debug);
         cli::init()
      }
      Err(e) => {
         error!("Failed to load config file: {}", e);
      }
   }

}

