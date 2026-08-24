// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use chrono::{DateTime, Utc};

// Rough human-friendly time delta. "2 months ago" is close enough when months
// are about 30 days.
pub(crate) fn format_relative_time(timestamp: Option<DateTime<Utc>>) -> String {
    let Some(timestamp) = timestamp else {
        return "Never played".to_string();
    };
    let seconds = Utc::now()
        .signed_duration_since(timestamp)
        .num_seconds()
        .max(0) as u64;
    match seconds {
        0..=59 => "Just now".to_string(),
        60..=3599 => ago(seconds / 60, "minute"),
        3600..=86399 => ago(seconds / 3600, "hour"),
        86400..=2591999 => ago(seconds / 86400, "day"),
        2592000..=31535999 => ago(seconds / 2592000, "month"),
        _ => "Over a year ago".to_string(),
    }
}

fn ago(value: u64, unit: &str) -> String {
    let plural = if value == 1 { "" } else { "s" };
    format!("{value} {unit}{plural} ago")
}
