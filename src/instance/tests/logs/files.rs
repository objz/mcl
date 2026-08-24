// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn create_log_file_name_has_millisecond_precision() {
    let tmp = tempfile::tempdir().unwrap();
    let path = create_log_file(tmp.path(), "world").unwrap();
    let name = path.file_name().unwrap().to_str().unwrap();
    assert!(name.ends_with(".log"), "name={name}");
    // stem is 22 chars: %Y-%m-%d_%H-%M-%S%3f
    let stem = name.strip_suffix(".log").unwrap();
    assert_eq!(stem.len(), 22, "name={name}");
    assert!(
        stem[stem.len() - 3..].chars().all(|c| c.is_ascii_digit()),
        "name={name}"
    );

    // pin the pattern: %3f must be zero-padded so names stay sortable
    let dt =
        chrono::NaiveDateTime::parse_from_str("2025-01-02T15:04:05.005", "%Y-%m-%dT%H:%M:%S%.3f")
            .unwrap();
    assert_eq!(
        dt.format("%Y-%m-%d_%H-%M-%S%3f").to_string(),
        "2025-01-02_15-04-05005"
    );
}

#[test]
fn log_dir_builds_correct_path() {
    let p = log_dir(Path::new("/instances"), "my-world");
    assert_eq!(
        p,
        PathBuf::from("/instances/my-world/minecraft/logs/launches")
    );
}
