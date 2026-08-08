// in-memory ring buffer for stdout/stderr from running mc instances.
// capped at 2000 lines per instance so it doesn't eat all the RAM
// if someone leaves a server running for a week. you're welcome.

use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};

use crate::instance::launch::parser::{LogLevel, LogStream, ParsedLogEvent};

const MAX_LINES: usize = 2000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveLogLine {
    pub level: LogLevel,
    pub stream: LogStream,
    pub text: String,
}

type LogsMap = Arc<Mutex<HashMap<String, VecDeque<LiveLogLine>>>>;
pub static LOGS: LazyLock<LogsMap> = LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

pub fn push(name: &str, line: impl Into<String>) {
    push_line(name, LogLevel::Info, LogStream::Stdout, line);
}

pub fn push_event(name: &str, event: ParsedLogEvent) {
    for line in event.lines {
        push_line(name, event.level, event.primary_stream, line);
    }
}

pub fn push_line(name: &str, level: LogLevel, stream: LogStream, line: impl Into<String>) {
    if let Ok(mut logs) = LOGS.lock() {
        let buf = logs.entry(name.to_string()).or_insert_with(VecDeque::new);
        buf.push_back(LiveLogLine {
            level,
            stream,
            text: line.into(),
        });
        while buf.len() > MAX_LINES {
            buf.pop_front();
        }
    }
}

pub fn get_entries(name: &str) -> Vec<LiveLogLine> {
    LOGS.lock()
        .ok()
        .and_then(|logs| logs.get(name).map(|buf| buf.iter().cloned().collect()))
        .unwrap_or_default()
}

pub fn get_all(name: &str) -> Vec<String> {
    LOGS.lock()
        .ok()
        .and_then(|logs| {
            logs.get(name)
                .map(|buf| buf.iter().map(|line| line.text.clone()).collect())
        })
        .unwrap_or_default()
}

pub fn clear(name: &str) {
    if let Ok(mut logs) = LOGS.lock() {
        logs.remove(name);
    }
}

#[cfg(test)]
#[path = "../tests/logs/live.rs"]
mod tests;
