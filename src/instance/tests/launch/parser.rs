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
