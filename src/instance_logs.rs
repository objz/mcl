use once_cell::sync::Lazy;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

const MAX_LINES: usize = 2000;

type LogsMap = Arc<Mutex<HashMap<String, VecDeque<String>>>>;
pub static LOGS: Lazy<LogsMap> = Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

pub fn push(name: &str, line: impl Into<String>) {
    if let Ok(mut logs) = LOGS.lock() {
        let buf = logs.entry(name.to_string()).or_insert_with(VecDeque::new);
        buf.push_back(line.into());
        while buf.len() > MAX_LINES {
            buf.pop_front();
        }
    }
}

pub fn get_all(name: &str) -> Vec<String> {
    LOGS.lock()
        .ok()
        .and_then(|logs| logs.get(name).map(|buf| buf.iter().cloned().collect()))
        .unwrap_or_default()
}

pub fn clear(name: &str) {
    if let Ok(mut logs) = LOGS.lock() {
        logs.remove(name);
    }
}
