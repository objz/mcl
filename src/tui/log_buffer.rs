use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;
use tracing::Level;

const MAX_LOG_ENTRIES: usize = 200;

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: Level,
    pub message: String,
}

pub static LOG_BUFFER: Lazy<Arc<Mutex<VecDeque<LogEntry>>>> =
    Lazy::new(|| Arc::new(Mutex::new(VecDeque::new())));

pub fn push_log(entry: LogEntry) {
    match LOG_BUFFER.lock() {
        Ok(mut buf) => {
            buf.push_back(entry);
            while buf.len() > MAX_LOG_ENTRIES {
                buf.pop_front();
            }
        }
        Err(e) => {
            tracing::error!("Log buffer lock poisoned: {}", e);
        }
    }
}

/// Returns all log entries newest-first.
pub fn get_logs() -> Vec<LogEntry> {
    match LOG_BUFFER.lock() {
        Ok(buf) => buf.iter().rev().cloned().collect(),
        Err(_) => Vec::new(),
    }
}
