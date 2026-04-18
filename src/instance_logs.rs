use std::collections::{HashMap, VecDeque};
use std::sync::LazyLock;
use std::sync::{Arc, Mutex};

const MAX_LINES: usize = 2000;

type LogsMap = Arc<Mutex<HashMap<String, VecDeque<String>>>>;
pub static LOGS: LazyLock<LogsMap> = LazyLock::new(|| Arc::new(Mutex::new(HashMap::new())));

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_get_all() {
        let name = "test_push_get";
        push(name, "line1");
        push(name, "line2");
        let lines = get_all(name);
        assert!(lines.contains(&"line1".to_string()));
        assert!(lines.contains(&"line2".to_string()));
    }

    #[test]
    fn get_all_missing_instance_returns_empty() {
        let lines = get_all("nonexistent_instance_xyz");
        assert!(lines.is_empty());
    }

    #[test]
    fn clear_removes_instance() {
        let name = "test_clear";
        push(name, "data");
        assert!(!get_all(name).is_empty());
        clear(name);
        assert!(get_all(name).is_empty());
    }

    #[test]
    fn clear_nonexistent_is_noop() {
        clear("never_existed_xyz");
    }

    #[test]
    fn buffer_respects_max_lines() {
        let name = "test_max_lines";
        for i in 0..(MAX_LINES + 100) {
            push(name, format!("line-{i}"));
        }
        let lines = get_all(name);
        assert_eq!(lines.len(), MAX_LINES);
        assert!(lines
            .last()
            .unwrap()
            .contains(&format!("{}", MAX_LINES + 99)));
    }
}
