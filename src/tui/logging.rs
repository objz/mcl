use std::fmt;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::{Level, Subscriber};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, Layer};

use std::sync::LazyLock;

use crate::tui::error_buffer::{self, ErrorEvent};

static APP_LOG_LINES: LazyLock<Arc<Mutex<Vec<String>>>> =
    LazyLock::new(|| Arc::new(Mutex::new(Vec::new())));

pub fn get_app_logs() -> Vec<String> {
    APP_LOG_LINES.lock().map(|l| l.clone()).unwrap_or_default()
}

fn push_app_log(line: String) {
    if let Ok(mut lines) = APP_LOG_LINES.lock() {
        lines.push(line);
        if lines.len() > 5000 {
            let drain = lines.len() - 5000;
            lines.drain(..drain);
        }
    }
}

pub fn init() -> WorkerGuard {
    let log_dir = match dirs_next::cache_dir() {
        Some(d) => d.join("mcl"),
        None => std::path::PathBuf::from("./cache"),
    };
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

    let rust_log = std::env::var("RUST_LOG").unwrap_or_default().to_lowercase();
    let tui_level = if rust_log.contains("debug") || rust_log.contains("trace") {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };

    match tui_logger::init_logger(log::LevelFilter::Debug) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Warning: tui-logger init failed: {}", e);
        }
    }
    tui_logger::set_default_level(tui_level);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_filter(env_filter),
        )
        .with(tui_logger::TuiTracingSubscriberLayer)
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
        let target = event.metadata().target();

        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        let level_str = match level {
            Level::ERROR => "ERROR",
            Level::WARN => "WARN",
            Level::INFO => "INFO",
            Level::DEBUG => "DEBUG",
            Level::TRACE => "TRACE",
        };
        let now = chrono::Local::now().format("%H:%M:%S");
        push_app_log(format!("{now}:{level_str}:{target}: {}", visitor.message));

        if level <= Level::WARN {
            error_buffer::push_error(ErrorEvent {
                id: 0,
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
