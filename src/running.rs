use chrono::{DateTime, Utc};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
pub enum RunState {
    Starting,
    Running,
    Crashed(Option<i32>),
}

pub static RUNNING: Lazy<Arc<Mutex<HashMap<String, RunState>>>> =
    Lazy::new(|| Arc::new(Mutex::new(HashMap::new())));

pub static PENDING_LAST_PLAYED: Lazy<Arc<Mutex<Vec<(String, DateTime<Utc>)>>>> =
    Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

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
