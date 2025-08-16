use std::fmt;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::{Level, Subscriber};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, Layer};

use crate::config::get_config_path;
use crate::tui::error_buffer::{self, ErrorEvent};

pub fn init() -> WorkerGuard {
    let log_dir = get_config_path();
    match std::fs::create_dir_all(&log_dir) {
        Ok(_) => {}
        Err(e) => {
            eprintln!(
                "Warning: failed to create log directory {}: {}",
                log_dir.display(),
                e
            );
        }
    }

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
        .with(StatusLayer::new(error_buffer::ERROR_EVENTS.clone()))
        .init();

    guard
}

struct StatusLayer;

impl StatusLayer {
    fn new(_events: Arc<Mutex<std::collections::VecDeque<ErrorEvent>>>) -> Self {
        Self
    }
}

impl<S: Subscriber> Layer<S> for StatusLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let level = *event.metadata().level();

        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        crate::tui::log_buffer::push_log(crate::tui::log_buffer::LogEntry {
            level,
            message: visitor.message.clone(),
        });

        if level <= Level::WARN {
            error_buffer::push_error(ErrorEvent {
                level,
                message: visitor.message,
                pushed_at: std::time::Instant::now(),
            });
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
            self.message = formatted
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(&formatted)
                .to_string();
        }
    }
}
