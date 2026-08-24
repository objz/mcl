// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// integration tests for the public log files API.
// these tests touch the filesystem and exercise the module as an external
// consumer would.

use std::path::{Path, PathBuf};

use rmcl::instance::logs::files::{create_log_file, log_dir, read_log_file, scan_log_files};

fn setup_log_dir(tmp: &Path, instance: &str) -> PathBuf {
    let dir = log_dir(tmp, instance);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn scan_log_files_empty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    setup_log_dir(tmp.path(), "inst");
    let entries = scan_log_files(tmp.path(), "inst");
    assert!(entries.is_empty());
}

#[test]
fn scan_log_files_finds_logs() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_log_dir(tmp.path(), "inst");
    std::fs::write(dir.join("2024-01-01_12-00-00.log"), "line1\nline2").unwrap();
    std::fs::write(dir.join("2024-01-02_12-00-00.log"), "line3").unwrap();
    let entries = scan_log_files(tmp.path(), "inst");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "2024-01-02_12-00-00.log");
    assert_eq!(entries[1].name, "2024-01-01_12-00-00.log");
}

#[test]
fn scan_log_files_ignores_non_log() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_log_dir(tmp.path(), "inst");
    std::fs::write(dir.join("notes.txt"), "not a log").unwrap();
    std::fs::write(dir.join("real.log"), "log line").unwrap();
    let entries = scan_log_files(tmp.path(), "inst");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "real.log");
}

#[test]
fn scan_log_files_missing_dir_returns_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let entries = scan_log_files(tmp.path(), "ghost");
    assert!(entries.is_empty());
}

#[test]
fn read_log_file_returns_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.log");
    std::fs::write(&path, "alpha\nbeta\ngamma").unwrap();
    let lines = read_log_file(&path);
    assert_eq!(lines, vec!["alpha", "beta", "gamma"]);
}

#[test]
fn read_log_file_missing_returns_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let lines = read_log_file(&tmp.path().join("missing.log"));
    assert!(lines.is_empty());
}

#[test]
fn create_log_file_creates_path() {
    let tmp = tempfile::tempdir().unwrap();
    let path = create_log_file(tmp.path(), "inst");
    assert!(path.is_some());
    let path = path.unwrap();
    assert!(path.to_string_lossy().ends_with(".log"));
    assert!(path.parent().unwrap().exists());
}
