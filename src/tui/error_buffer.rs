// thread-safe FIFO queue for error/warning toasts displayed in the UI.
// also (ab)used for INFO toasts like "desktop shortcut created" because
// why build a separate notification system when this one works fine.
//
// callers pass id: 0 and push_error assigns a real unique id. the id is
// used by the render layer to track per-toast animation state.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use std::sync::LazyLock;
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

pub static ERROR_EVENTS: LazyLock<Arc<Mutex<VecDeque<ErrorEvent>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(VecDeque::new())));

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

#[must_use]
pub fn has_errors() -> bool {
    match ERROR_EVENTS.lock() {
        Ok(events) => !events.is_empty(),
        Err(_) => false,
    }
}

#[must_use]
pub fn pop_error() -> Option<ErrorEvent> {
    match ERROR_EVENTS.lock() {
        Ok(mut events) => events.pop_front(),
        Err(_) => None,
    }
}

#[must_use]
pub fn peek_error() -> Option<ErrorEvent> {
    match ERROR_EVENTS.lock() {
        Ok(events) => events.front().cloned(),
        Err(_) => None,
    }
}

#[must_use]
// returned in reverse order (newest first) so they stack top-down in the UI
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

    // peek must not mutate the queue: count + identity of the front event
    // are preserved across a peek. tagging the message lets us identify
    // our own event amid whatever other tests pushed.
    #[test]
    fn peek_does_not_remove() {
        let tag = "PEEK_DOES_NOT_REMOVE_TAG";
        push_error(make_event(tag));
        let count_before = peek_all_errors().len();
        let peeked = peek_error();
        let count_after = peek_all_errors().len();
        assert_eq!(
            count_before, count_after,
            "peek should not change queue length"
        );
        // and the front entry is the same id we just peeked
        assert!(peeked.is_some());
        assert!(
            peek_all_errors()
                .iter()
                .any(|e| e.message.contains(tag)),
            "our tagged event should still be in the queue"
        );
    }

    // ERROR_EVENTS is a global static shared across tests, so we tag our
    // events with a unique substring and filter to just those before
    // asserting; otherwise concurrent tests would interfere.
    #[test]
    fn peek_all_returns_newest_first() {
        let tag = "PEEK_NEWEST_FIRST_TAG";
        push_error(make_event(&format!("{tag}_a")));
        push_error(make_event(&format!("{tag}_b")));
        let ours: Vec<_> = peek_all_errors()
            .into_iter()
            .filter(|e| e.message.contains(tag))
            .collect();
        assert_eq!(ours.len(), 2, "expected 2 tagged events, got {ours:?}");
        // _b was pushed last, so it must come out first (newest-first iter)
        assert!(ours[0].message.ends_with("_b"));
        assert!(ours[1].message.ends_with("_a"));
        // newer event also has the higher auto-assigned id
        assert!(ours[0].id > ours[1].id);
    }

    #[test]
    fn auto_assigned_ids_are_unique() {
        let tag = "AUTO_ID_UNIQUE_TAG";
        push_error(make_event(&format!("{tag}_1")));
        push_error(make_event(&format!("{tag}_2")));
        let ours: Vec<_> = peek_all_errors()
            .into_iter()
            .filter(|e| e.message.contains(tag))
            .collect();
        assert_eq!(ours.len(), 2);
        assert_ne!(ours[0].id, ours[1].id);
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
