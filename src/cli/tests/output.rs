// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::{format_datetime, render_table};
use chrono::{TimeZone, Utc};

#[test]
fn render_table_aligns_columns() {
    let rendered = render_table(
        &["Name", "State"],
        &[
            vec!["Alpha".to_string(), "enabled".to_string()],
            vec!["Longer Name".to_string(), "off".to_string()],
        ],
    );

    let lines: Vec<&str> = rendered.lines().collect();
    assert_eq!(lines[0], "Name         State  ");
    assert_eq!(lines[1], "-----------  -------");
    assert_eq!(lines[2], "Alpha        enabled");
    assert_eq!(lines[3], "Longer Name  off    ");
}

#[test]
fn render_table_aligns_multibyte_names() {
    // padding must use display width, not byte length: "Öctra" is 6 bytes
    // but 5 columns wide, and byte-based padding pushed every later column.
    let rendered = render_table(
        &["Name", "State"],
        &[
            vec!["Öctra".to_string(), "enabled".to_string()],
            vec!["Longer Name".to_string(), "off".to_string()],
        ],
    );

    let lines: Vec<&str> = rendered.lines().collect();
    assert_eq!(lines[0], "Name         State  ");
    assert_eq!(lines[2], "Öctra        enabled");
    assert_eq!(lines[3], "Longer Name  off    ");
}

#[test]
fn formats_datetime_consistently() {
    let dt = Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
    assert_eq!(format_datetime(&dt), "2024-01-02 03:04:05 UTC");
}
