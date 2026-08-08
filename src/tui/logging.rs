// sets up the tracing subscriber stack: file logging, tui-logger widget, and
// our custom StatusLayer that feeds WARN/ERROR events into the error toast system.
// also keeps an in-memory ring buffer of log lines for the log overlay viewer.

use std::fmt;
use std::path::Path;
use std::sync::{Arc, Mutex};

use tracing::field::{Field, Visit};
use tracing::{Level, Subscriber};
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, Layer};

use std::sync::LazyLock;

use crate::feedback::errors::{self as error_buffer, ErrorEvent};

const MINECRAFT_LOG_TARGET: &str = "mc_instance";
const DEFAULT_FILE_FILTER: &str = "warn,rmcl=trace";

// capped at 5000 lines so it doesn't eat all the ram if something goes haywire
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

// returns a WorkerGuard that must be held alive for the duration of the
// program, otherwise the file logging thread gets dropped immediately.
// yes, you will spend 30 minutes debugging "why aren't my logs writing"
// before you remember this. ask me how i know.
pub fn init() -> WorkerGuard {
    let log_dir = match dirs_next::cache_dir() {
        Some(d) => d.join("rmcl"),
        None => std::path::PathBuf::from("./cache"),
    };

    let (non_blocking, guard) = open_log_writer(&log_dir);

    let file_filter =
        EnvFilter::try_from_env("RUST_LOG").unwrap_or_else(|_| EnvFilter::new(DEFAULT_FILE_FILTER));
    let file_filter =
        file_filter.add_directive(format!("{MINECRAFT_LOG_TARGET}=off").parse().unwrap());

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
                .with_filter(file_filter),
        )
        .with(
            tui_logger::TuiTracingSubscriberLayer.with_filter(filter_fn(|metadata| {
                should_record_app_log(metadata.target(), *metadata.level())
            })),
        )
        .with(
            StatusLayer::new(error_buffer::ERROR_EVENTS.clone()).with_filter(filter_fn(
                |metadata| should_record_app_log(metadata.target(), *metadata.level()),
            )),
        )
        .init();

    guard
}

fn open_log_writer(log_dir: &Path) -> (NonBlocking, WorkerGuard) {
    let now = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    let file = std::fs::create_dir_all(log_dir).and_then(|_| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_dir.join(format!("rmcl_{now}.log")))
    });

    match file {
        Ok(file) => tracing_appender::non_blocking(file),
        Err(error) => {
            eprintln!(
                "Warning: failed to open log file in {}: {}",
                log_dir.display(),
                error
            );
            tracing_appender::non_blocking(std::io::sink())
        }
    }
}

// custom tracing layer that intercepts all log events:
// - everything gets appended to the in-memory log buffer for the overlay viewer
// - WARN and ERROR additionally get pushed as error toasts
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
        if should_record_app_log(target, level) {
            push_app_log(format!("{now}:{level_str}:{target}: {}", visitor.message));
        }

        if level <= Level::WARN && should_record_app_log(target, level) {
            error_buffer::push_error(ErrorEvent {
                id: 0,
                level,
                message: visitor.message,
                pushed_at: std::time::Instant::now(),
            });
        }
    }
}

fn should_record_app_log(target: &str, level: Level) -> bool {
    if target == MINECRAFT_LOG_TARGET {
        return false;
    }
    if is_svg_renderer_target(target) {
        return false;
    }

    target == "rmcl" || target.starts_with("rmcl::") || level <= Level::WARN
}

fn is_svg_renderer_target(target: &str) -> bool {
    ["usvg", "resvg", "tiny_skia", "fontdb"]
        .iter()
        .any(|renderer| target == *renderer || target.starts_with(&format!("{renderer}::")))
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
            // Debug format wraps strings in quotes, strip them so the toast
            // doesn't show "\"something went wrong\""
            let formatted = format!("{:?}", value);
            self.message = formatted
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .unwrap_or(&formatted)
                .to_string();
        }
    }
}

#[cfg(test)]
#[path = "tests/logging.rs"]
mod tests;
