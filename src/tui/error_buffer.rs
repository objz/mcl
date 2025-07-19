use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use once_cell::sync::Lazy;
use tracing::Level;

const MAX_ERROR_EVENTS: usize = 50;

#[derive(Debug, Clone)]
pub struct ErrorEvent {
    pub level: Level,
    pub message: String,
    pub pushed_at: Instant,
}

pub static ERROR_EVENTS: Lazy<Arc<Mutex<VecDeque<ErrorEvent>>>> =
    Lazy::new(|| Arc::new(Mutex::new(VecDeque::new())));

pub fn push_error(event: ErrorEvent) {
    match ERROR_EVENTS.lock() {
        Ok(mut events) => {
            events.push_back(event);
            while events.len() > MAX_ERROR_EVENTS {
                events.pop_front();
            }
        }
        Err(e) => {
            tracing::error!("Error buffer lock poisoned: {}", e);
        }
    }
}

pub fn has_errors() -> bool {
    match ERROR_EVENTS.lock() {
        Ok(events) => !events.is_empty(),
        Err(_) => false,
    }
}

pub fn pop_error() -> Option<ErrorEvent> {
    match ERROR_EVENTS.lock() {
        Ok(mut events) => events.pop_front(),
        Err(_) => None,
    }
}

pub fn peek_error() -> Option<ErrorEvent> {
    match ERROR_EVENTS.lock() {
        Ok(events) => events.front().cloned(),
        Err(_) => None,
    }
}

/// Returns all queued error events, newest first (for top-to-bottom stacking).
pub fn peek_all_errors() -> Vec<ErrorEvent> {
    match ERROR_EVENTS.lock() {
        Ok(events) => events.iter().rev().cloned().collect(),
        Err(_) => Vec::new(),
    }
}
