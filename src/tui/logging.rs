use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;
use tracing::field::{Field, Visit};
use tracing::{Level, Subscriber};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, Layer};

use crate::config::get_config_path;

const MAX_STATUS_EVENTS: usize = 50;

#[derive(Debug, Clone)]
pub struct StatusEvent {
    pub level: Level,
    pub message: String,
}

pub static STATUS_EVENTS: Lazy<Arc<Mutex<VecDeque<StatusEvent>>>> =
    Lazy::new(|| Arc::new(Mutex::new(VecDeque::new())));

pub fn init() -> WorkerGuard {
    let log_dir = get_config_path();
    std::fs::create_dir_all(&log_dir).expect("Failed to create log directory");

    let file_appender = tracing_appender::rolling::daily(&log_dir, "mcl.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::builder()
        .with_default_directive(Level::INFO.into())
        .from_env_lossy();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_filter(env_filter),
        )
        .with(StatusLayer::new(STATUS_EVENTS.clone()))
        .init();

    guard
}

struct StatusLayer {
    events: Arc<Mutex<VecDeque<StatusEvent>>>,
}

impl StatusLayer {
    fn new(events: Arc<Mutex<VecDeque<StatusEvent>>>) -> Self {
        Self { events }
    }
}

impl<S: Subscriber> Layer<S> for StatusLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let level = *event.metadata().level();

        // Only capture errors and warnings for the status bar
        if level > Level::WARN {
            return;
        }

        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        if let Ok(mut events) = self.events.lock() {
            events.push_back(StatusEvent {
                level,
                message: visitor.message,
            });
            while events.len() > MAX_STATUS_EVENTS {
                events.pop_front();
            }
        }
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            let formatted = format!("{:?}", value);
            // fmt::Arguments Debug wraps in quotes — strip them
            self.message = formatted
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(&formatted)
                .to_string();
        }
    }
}
