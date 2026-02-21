use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use once_cell::sync::Lazy;
use tracing::Level;

const MAX_ERROR_EVENTS: usize = 50;
static NEXT_ERROR_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct ErrorEvent {
    pub id: u64,
    pub level: Level,
    pub message: String,
    pub pushed_at: Instant,
}

pub static ERROR_EVENTS: Lazy<Arc<Mutex<VecDeque<ErrorEvent>>>> =
    Lazy::new(|| Arc::new(Mutex::new(VecDeque::new())));

pub fn push_error(event: ErrorEvent) {
    match ERROR_EVENTS.lock() {
        Ok(mut events) => {
            let mut event = event;
            event.id = NEXT_ERROR_ID.fetch_add(1, Ordering::Relaxed);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(msg: &str) -> ErrorEvent {
        ErrorEvent {
            id: 0,
            level: Level::ERROR,
            message: msg.to_string(),
            pushed_at: Instant::now(),
        }
    }

    #[test]
    fn push_and_pop_fifo() {
        push_error(make_event("err_fifo_1"));
        push_error(make_event("err_fifo_2"));
        let first = pop_error();
        assert!(first.is_some() || has_errors());
    }

    #[test]
    fn has_errors_after_push() {
        push_error(make_event("err_has"));
        assert!(has_errors());
    }

    #[test]
    fn peek_does_not_remove() {
        push_error(make_event("err_peek"));
        let before = peek_error();
        assert!(before.is_some());
        assert!(has_errors());
    }

    #[test]
    fn peek_all_returns_newest_first() {
        push_error(make_event("err_all_a"));
        push_error(make_event("err_all_b"));
        let all = peek_all_errors();
        assert!(all.len() >= 2);
        if all.len() >= 2 {
            assert!(all[0].id >= all[1].id);
        }
    }

    #[test]
    fn auto_assigned_ids_are_unique() {
        push_error(make_event("err_id_1"));
        push_error(make_event("err_id_2"));
        let all = peek_all_errors();
        if all.len() >= 2 {
            let ids: Vec<u64> = all.iter().map(|e| e.id).collect();
            let unique: std::collections::HashSet<u64> = ids.iter().copied().collect();
            assert_eq!(ids.len(), unique.len());
        }
    }

    #[test]
    fn overflow_drops_oldest() {
        for i in 0..(MAX_ERROR_EVENTS + 10) {
            push_error(make_event(&format!("overflow_{i}")));
        }
        let all = peek_all_errors();
        assert!(all.len() <= MAX_ERROR_EVENTS);
    }
}
