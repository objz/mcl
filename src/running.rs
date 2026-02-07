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
