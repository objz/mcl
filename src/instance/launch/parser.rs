// live minecraft process log parser.
//
// disk launch logs stay raw, but the tui needs enough structure to color and
// group output sanely. this parser keeps stdout/stderr as a hint, frames
// multiline java/jvm/native bursts into events, then assigns a semantic level.

use libcasr::exception::Exception;
use libcasr::java::{JavaException, JavaStacktrace};
use libcasr::stacktrace::ParseStacktrace;
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JavaParse {
    pub has_exception: bool,
    pub has_stacktrace: bool,
    pub stacktrace_frames: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLogEvent {
    pub level: LogLevel,
    pub primary_stream: LogStream,
    pub lines: Vec<String>,
    pub java: Option<JavaParse>,
}

#[derive(Debug, Clone)]
struct PendingEvent {
    level: LogLevel,
    primary_stream: LogStream,
    lines: Vec<String>,
    normalized_lines: Vec<String>,
    kind: EventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventKind {
    Header,
    Java,
    JvmBurst,
    Native,
    Plain,
}

#[derive(Debug)]
pub struct MinecraftLogParser {
    current: Option<PendingEvent>,
    minecraft_header_re: Regex,
    plain_level_re: Regex,
    java_exception_re: Regex,
    java_continuation_re: Regex,
    jvm_burst_re: Regex,
    native_header_re: Regex,
}

impl Default for MinecraftLogParser {
    fn default() -> Self {
        Self::new()
    }
}

impl MinecraftLogParser {
    pub fn new() -> Self {
        Self {
            // header matching stays local because this parser is in the live
            // path; pulling in heavier pattern machinery would not help with
            // the multiline framing problem.
            minecraft_header_re: Regex::new(
                r"(?i)^\s*(?:\[[^\]]+\]\s*)*\[[^/\]]+/(?P<level>TRACE|DEBUG|INFO|WARN|ERROR|FATAL)\]\s*:",
            )
            .expect("valid minecraft header regex"),
            plain_level_re: Regex::new(
                r"(?i)^\s*(?:\[[^\]]+\]\s*)*(?P<level>TRACE|DEBUG|INFO|WARN|ERROR|FATAL)\b\s*(?:[:\]\-])?",
            )
            .expect("valid plain level regex"),
            java_exception_re: Regex::new(
                r"(?i)^\s*(?:Exception in thread .+|Caused by: |Suppressed: )?(?:[a-z_$][\w$]*\.)+(?:[A-Z][\w$]*(?:Exception|Error)|Throwable)(?::|\b)",
            )
            .expect("valid java exception regex"),
            java_continuation_re: Regex::new(
                r"^(?:\s+at\s+.+|\s*\.\.\.\s+\d+\s+more|Caused by:\s+.+|Suppressed:\s+.+)$",
            )
            .expect("valid java continuation regex"),
            jvm_burst_re: Regex::new(
                r"(?i)^\s*(?:Error:\s+.+|Unrecognized option:\s+.+|Could not create the Java Virtual Machine\.|A fatal exception has occurred\. Program will exit\.|Invalid maximum heap size:.+)$",
            )
            .expect("valid jvm burst regex"),
            native_header_re: Regex::new(r"^\[[A-Z]\]\s+\[[0-9:.]+\]\s+[^:]+:")
                .expect("valid native header regex"),
            current: None,
        }
    }

    pub fn push_line(&mut self, stream: LogStream, line: impl Into<String>) -> Vec<ParsedLogEvent> {
        let line = line.into();
        // classify against ansi-stripped text, but keep the original line for
        // display so the live log remains faithful to the child process.
        let normalized = fast_strip_ansi::strip_ansi_string(&line).into_owned();
        let analysis = self.analyze_line(stream, &normalized);

        let Some(current) = &mut self.current else {
            self.current = Some(PendingEvent::new(stream, line, normalized, analysis));
            return Vec::new();
        };

        // only hard starts split events. this keeps stacktraces and native
        // loader bursts together even when they arrive one line at a time.
        if should_split(current.kind, analysis.kind) {
            let finished = self.current.take().map(finish_event);
            self.current = Some(PendingEvent::new(stream, line, normalized, analysis));
            finished.into_iter().collect()
        } else {
            if analysis.level_priority() > current.level_priority() {
                current.level = analysis.level;
            }
            if matches!(current.kind, EventKind::Plain)
                && !matches!(analysis.kind, EventKind::Plain)
            {
                current.kind = analysis.kind;
            }
            current.lines.push(line);
            current.normalized_lines.push(normalized);
            Vec::new()
        }
    }

    pub fn flush(&mut self) -> Option<ParsedLogEvent> {
        self.current.take().map(finish_event)
    }

    pub fn has_pending(&self) -> bool {
        self.current.is_some()
    }

    fn analyze_line(&self, stream: LogStream, normalized: &str) -> LineAnalysis {
        if let Some(level) = self.header_level(normalized) {
            return LineAnalysis {
                level,
                kind: EventKind::Header,
            };
        }

        if self.java_exception_re.is_match(normalized) {
            return LineAnalysis {
                level: LogLevel::Error,
                kind: EventKind::Java,
            };
        }

        if self.java_continuation_re.is_match(normalized) {
            return LineAnalysis {
                level: LogLevel::Error,
                kind: EventKind::Java,
            };
        }

        if self.jvm_burst_re.is_match(normalized) {
            return LineAnalysis {
                level: LogLevel::Error,
                kind: EventKind::JvmBurst,
            };
        }

        if self.native_header_re.is_match(normalized) {
            return LineAnalysis {
                level: native_level(normalized).unwrap_or(LogLevel::Warn),
                kind: EventKind::Native,
            };
        }

        LineAnalysis {
            level: match stream {
                LogStream::Stdout => LogLevel::Info,
                LogStream::Stderr => LogLevel::Error,
            },
            kind: EventKind::Plain,
        }
    }

    fn header_level(&self, normalized: &str) -> Option<LogLevel> {
        self.minecraft_header_re
            .captures(normalized)
            .and_then(|caps| caps.name("level"))
            .or_else(|| {
                self.plain_level_re
                    .captures(normalized)
                    .and_then(|caps| caps.name("level"))
            })
            .and_then(|m| parse_level(m.as_str()))
    }
}

#[derive(Debug, Clone, Copy)]
struct LineAnalysis {
    level: LogLevel,
    kind: EventKind,
}

impl LineAnalysis {
    fn level_priority(self) -> u8 {
        level_priority(self.level)
    }
}

impl PendingEvent {
    fn new(stream: LogStream, line: String, normalized: String, analysis: LineAnalysis) -> Self {
        Self {
            level: analysis.level,
            primary_stream: stream,
            lines: vec![line],
            normalized_lines: vec![normalized],
            kind: analysis.kind,
        }
    }

    fn level_priority(&self) -> u8 {
        level_priority(self.level)
    }
}

fn should_split(current: EventKind, next: EventKind) -> bool {
    match next {
        EventKind::Header => true,
        EventKind::Native => !matches!(current, EventKind::Native),
        EventKind::JvmBurst => !matches!(current, EventKind::JvmBurst),
        EventKind::Java => !matches!(current, EventKind::Java | EventKind::Header),
        EventKind::Plain => false,
    }
}

fn finish_event(mut event: PendingEvent) -> ParsedLogEvent {
    let normalized_body = event.normalized_lines.join("\n");
    // libcasr is intentionally second-stage here: first frame the multiline
    // event ourselves, then ask libcasr whether the block is a java failure.
    let java = parse_java(&normalized_body);

    if java
        .as_ref()
        .is_some_and(|java| java.has_exception || java.has_stacktrace)
    {
        event.level = LogLevel::Error;
    }

    ParsedLogEvent {
        level: event.level,
        primary_stream: event.primary_stream,
        lines: event.lines,
        java,
    }
}

fn parse_java(body: &str) -> Option<JavaParse> {
    let stacktrace_entries = JavaStacktrace::extract_stacktrace(body).unwrap_or_default();
    let parsed_stacktrace = if stacktrace_entries.is_empty() {
        None
    } else {
        JavaStacktrace::parse_stacktrace(&stacktrace_entries).ok()
    };
    let has_exception = JavaException::parse_exception(body).is_some();
    let has_stacktrace = parsed_stacktrace
        .as_ref()
        .is_some_and(|stacktrace| !stacktrace.is_empty());

    if has_exception || has_stacktrace {
        Some(JavaParse {
            has_exception,
            has_stacktrace,
            stacktrace_frames: parsed_stacktrace.map_or(0, |stacktrace| stacktrace.len()),
        })
    } else {
        None
    }
}

fn parse_level(level: &str) -> Option<LogLevel> {
    match level.to_ascii_uppercase().as_str() {
        "TRACE" => Some(LogLevel::Trace),
        "DEBUG" => Some(LogLevel::Debug),
        "INFO" => Some(LogLevel::Info),
        "WARN" => Some(LogLevel::Warn),
        "ERROR" | "FATAL" => Some(LogLevel::Error),
        _ => None,
    }
}

fn native_level(line: &str) -> Option<LogLevel> {
    match line.chars().nth(1)? {
        'E' | 'F' => Some(LogLevel::Error),
        'W' => Some(LogLevel::Warn),
        'I' => Some(LogLevel::Info),
        'D' => Some(LogLevel::Debug),
        _ => None,
    }
}

fn level_priority(level: LogLevel) -> u8 {
    match level {
        LogLevel::Trace => 0,
        LogLevel::Debug => 1,
        LogLevel::Info => 2,
        LogLevel::Warn => 3,
        LogLevel::Error => 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(lines: &[(LogStream, &str)]) -> Vec<ParsedLogEvent> {
        let mut parser = MinecraftLogParser::new();
        let mut events = Vec::new();
        for (stream, line) in lines {
            events.extend(parser.push_line(*stream, *line));
        }
        events.extend(parser.flush());
        events
    }

    #[test]
    fn classifies_minecraft_headers() {
        let events = parse_all(&[
            (LogStream::Stdout, "[Render thread/INFO]: hello"),
            (LogStream::Stdout, "[Render thread/WARN]: careful"),
            (LogStream::Stdout, "[Render thread/ERROR]: broken"),
            (LogStream::Stdout, "[Render thread/DEBUG]: noisy"),
            (LogStream::Stdout, "[Render thread/TRACE]: tiny"),
        ]);

        assert_eq!(events.len(), 5);
        assert_eq!(events[0].level, LogLevel::Info);
        assert_eq!(events[1].level, LogLevel::Warn);
        assert_eq!(events[2].level, LogLevel::Error);
        assert_eq!(events[3].level, LogLevel::Debug);
        assert_eq!(events[4].level, LogLevel::Trace);
    }

    #[test]
    fn explicit_stderr_info_stays_info() {
        let events = parse_all(&[(LogStream::Stderr, "[Render thread/INFO]: hello")]);

        assert_eq!(events[0].level, LogLevel::Info);
    }

    #[test]
    fn unstructured_stderr_falls_back_to_error() {
        let events = parse_all(&[(LogStream::Stderr, "native library failed")]);

        assert_eq!(events[0].level, LogLevel::Error);
    }

    #[test]
    fn groups_java_stacktrace() {
        let events = parse_all(&[
            (
                LogStream::Stderr,
                "Exception in thread \"main\" java.lang.RuntimeException: boom",
            ),
            (
                LogStream::Stderr,
                "\tat net.minecraft.client.Main.main(Main.java:42)",
            ),
            (
                LogStream::Stderr,
                "Caused by: java.lang.IllegalStateException: bad",
            ),
            (LogStream::Stderr, "\tat example.Mod.load(Mod.java:7)"),
            (LogStream::Stdout, "[Render thread/INFO]: after"),
        ]);

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].level, LogLevel::Error);
        assert_eq!(events[0].lines.len(), 4);
        assert!(
            events[0]
                .java
                .as_ref()
                .is_some_and(|java| java.has_stacktrace)
        );
        assert_eq!(events[1].level, LogLevel::Info);
    }

    #[test]
    fn groups_jvm_startup_failure_burst() {
        let events = parse_all(&[
            (LogStream::Stderr, "Unrecognized option: --bad"),
            (
                LogStream::Stderr,
                "Could not create the Java Virtual Machine.",
            ),
            (
                LogStream::Stderr,
                "A fatal exception has occurred. Program will exit.",
            ),
        ]);

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].level, LogLevel::Error);
        assert_eq!(events[0].lines.len(), 3);
    }

    #[test]
    fn colored_header_classifies_but_keeps_original_text() {
        let line = "\u{1b}[31m[Render thread/ERROR]: red\u{1b}[0m";
        let events = parse_all(&[(LogStream::Stdout, line)]);

        assert_eq!(events[0].level, LogLevel::Error);
        assert_eq!(events[0].lines[0], line);
    }
}
