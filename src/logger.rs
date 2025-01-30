use std::sync::Mutex;
use chrono::Local;
use color_eyre::owo_colors::OwoColorize;
use lazy_static::lazy_static;

pub struct Logger {
    debug: bool,
}

impl Logger {
    pub fn init(debug: bool) {
        let mut logger = LOGGER.lock().unwrap();
        logger.debug = debug;
    }

    fn timestamp() -> String {
        Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
    }

    pub fn debug(&self, message: &str) {
        if self.debug {
            println!(
                "{} {} {}",
                Self::timestamp().dimmed(),
                "[DEBUG]".cyan(),
                message
            );
        }
    }

    pub fn info(&self, message: &str) {
        println!(
            "{} {} {}",
            Self::timestamp().dimmed(),
            "[INFO]".green(),
            message
        );
    }

    pub fn error(&self, message: &str) {
        eprintln!(
            "{} {} {}",
            Self::timestamp().dimmed(),
            "[ERROR]".red().bold(),
            message
        );
    }
}

lazy_static! {
    pub static ref LOGGER: Mutex<Logger> = Mutex::new(Logger { debug: false });
}

pub fn get_logger() -> std::sync::MutexGuard<'static, Logger> {
    LOGGER.lock().unwrap()
}