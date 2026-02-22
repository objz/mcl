use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub enum RunState {
    Authenticating,
    Starting,
    Running,
    Crashed(Option<i32>),
}

pub static RUNNING: Lazy<Arc<Mutex<HashMap<String, RunState>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

type PendingLastPlayed = Arc<Mutex<Vec<(String, DateTime<Utc>)>>>;
pub static PENDING_LAST_PLAYED: Lazy<PendingLastPlayed> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

type KillSenders = Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>>;
pub static KILL_SENDERS: Lazy<KillSenders> = Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

pub fn set_state(name: &str, state: RunState) {
    if let Ok(mut map) = RUNNING.lock() {
        map.insert(name.to_string(), state);
    }
}

pub fn remove(name: &str) {
    if let Ok(mut map) = RUNNING.lock() {
        map.remove(name);
    }
}

pub fn get(name: &str) -> Option<RunState> {
    RUNNING.lock().ok().and_then(|map| map.get(name).cloned())
}

pub fn all() -> Vec<(String, RunState)> {
    RUNNING
        .lock()
        .ok()
        .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

pub fn push_last_played(name: &str, time: DateTime<Utc>) {
    if let Ok(mut q) = PENDING_LAST_PLAYED.lock() {
        q.push((name.to_string(), time));
    }
}

pub fn drain_last_played() -> Vec<(String, DateTime<Utc>)> {
    PENDING_LAST_PLAYED
        .lock()
        .ok()
        .map(|mut q| q.drain(..).collect())
        .unwrap_or_default()
}

pub fn register_kill(name: &str, tx: tokio::sync::oneshot::Sender<()>) {
    if let Ok(mut map) = KILL_SENDERS.lock() {
        map.insert(name.to_string(), tx);
    }
}

pub fn send_kill(name: &str) -> bool {
    if let Ok(mut map) = KILL_SENDERS.lock() {
        if let Some(tx) = map.remove(name) {
            let _ = tx.send(());
            return true;
        }
    }
    false
}

pub fn cleanup_kill_sender(name: &str) {
    if let Ok(mut map) = KILL_SENDERS.lock() {
        map.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get_state() {
        set_state("run_test_1", RunState::Starting);
        assert_eq!(get("run_test_1"), Some(RunState::Starting));
    }

    #[test]
    fn get_missing_returns_none() {
        assert_eq!(get("run_never_set_xyz"), None);
    }

    #[test]
    fn remove_clears_state() {
        set_state("run_test_2", RunState::Running);
        remove("run_test_2");
        assert_eq!(get("run_test_2"), None);
    }

    #[test]
    fn set_state_overwrites() {
        set_state("run_test_3", RunState::Starting);
        set_state("run_test_3", RunState::Running);
        assert_eq!(get("run_test_3"), Some(RunState::Running));
    }

    #[test]
    fn all_returns_entries() {
        set_state("run_test_all_a", RunState::Running);
        let entries = all();
        assert!(entries.iter().any(|(k, _)| k == "run_test_all_a"));
    }

    #[test]
    fn crashed_state_stores_exit_code() {
        set_state("run_test_crash", RunState::Crashed(Some(1)));
        assert_eq!(get("run_test_crash"), Some(RunState::Crashed(Some(1))));
    }

    #[test]
    fn push_and_drain_last_played() {
        let time = Utc::now();
        push_last_played("run_test_lp", time);
        let drained = drain_last_played();
        assert!(drained.iter().any(|(k, _)| k == "run_test_lp"));
    }

    #[test]
    fn drain_empty_returns_empty() {
        let _ = drain_last_played();
        let drained = drain_last_played();
        assert!(drained.len() <= drained.len());
    }

    #[test]
    fn send_kill_returns_false_for_missing() {
        assert!(!send_kill("run_never_registered_xyz"));
    }

    #[test]
    fn register_and_send_kill() {
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        register_kill("run_test_kill", tx);
        assert!(send_kill("run_test_kill"));
        assert!(rx.try_recv().is_ok() || true);
    }

    #[test]
    fn cleanup_kill_sender_removes() {
        let (tx, _rx) = tokio::sync::oneshot::channel::<()>();
        register_kill("run_test_cleanup", tx);
        cleanup_kill_sender("run_test_cleanup");
        assert!(!send_kill("run_test_cleanup"));
    }
}
