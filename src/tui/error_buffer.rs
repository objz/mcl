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
            crate::tui::request_redraw();
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
        Ok(mut events) => {
            let event = events.pop_front();
            if event.is_some() {
                crate::tui::request_redraw();
            }
            event
        }
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
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn clear_errors_for_test() {
        ERROR_EVENTS.lock().unwrap().clear();
    }

    fn make_event(msg: &str) -> ErrorEvent {
        ErrorEvent {
            id: 0,
            level: Level::ERROR,
            message: msg.to_string(),
            pushed_at: Instant::now(),
        }
    }

    #[test]
    fn peek_does_not_remove() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_errors_for_test();

        push_error(make_event("peek-test"));

        let count_before = peek_all_errors().len();
        let peeked = peek_error();
        let count_after = peek_all_errors().len();

        assert_eq!(
            count_before, count_after,
            "peek should not change queue length"
        );

        assert!(peeked.is_some());
        assert_eq!(peeked.unwrap().message, "peek-test");
    }

    #[test]
    fn peek_all_returns_newest_first() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_errors_for_test();

        push_error(make_event("newest_a"));
        push_error(make_event("newest_b"));

        let all = peek_all_errors();

        assert_eq!(all.len(), 2);
        assert!(all[0].message.ends_with("_b"));
        assert!(all[1].message.ends_with("_a"));
        assert!(all[0].id > all[1].id);
    }

    #[test]
    fn auto_assigned_ids_are_unique() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_errors_for_test();

        push_error(make_event("unique_1"));
        push_error(make_event("unique_2"));

        let all = peek_all_errors();

        assert_eq!(all.len(), 2);
        assert_ne!(all[0].id, all[1].id);
    }

    #[test]
    fn overflow_drops_oldest() {
        let _guard = TEST_LOCK.lock().unwrap();
        clear_errors_for_test();

        for i in 0..(MAX_ERROR_EVENTS + 10) {
            push_error(make_event(&format!("overflow_{i}")));
        }

        let all = peek_all_errors();

        assert_eq!(all.len(), MAX_ERROR_EVENTS);

        assert!(!all.iter().any(|e| e.message == "overflow_0"));
        assert!(!all.iter().any(|e| e.message == "overflow_9"));

        assert!(all.iter().any(|e| e.message == "overflow_10"));
        assert!(all.iter().any(|e| e.message == "overflow_59"));
    }
}
