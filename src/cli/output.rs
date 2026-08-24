// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// plain-text table rendering for CLI output. no fancy box-drawing,
// just left-aligned columns with two-space gaps. keeps it pipeable.
use chrono::{DateTime, Utc};
use ratatui::text::Span;

pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    print!("{}", render_table(headers, rows));
}

pub fn render_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let widths = column_widths(headers, rows);
    let mut out = String::new();

    out.push_str(&render_row(
        &headers
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>(),
        &widths,
    ));
    out.push('\n');
    out.push_str(&render_separator(&widths));
    out.push('\n');

    for row in rows {
        out.push_str(&render_row(row, &widths));
        out.push('\n');
    }

    out
}

pub fn format_datetime(value: &DateTime<Utc>) -> String {
    value.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

pub fn active_marker(active: bool) -> &'static str {
    if active { ">" } else { " " }
}

// display width (not byte length): instance and pack names are often
// non-ASCII, and padding by bytes misaligns every row below them.
fn display_width(value: &str) -> usize {
    Span::raw(value).width()
}

// find the widest value in each column to pad everything evenly
fn column_widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = headers.iter().map(|header| display_width(header)).collect();

    for row in rows {
        if row.len() > widths.len() {
            widths.resize(row.len(), 0);
        }

        for (index, value) in row.iter().enumerate() {
            widths[index] = widths[index].max(display_width(value));
        }
    }

    widths
}

fn render_row(row: &[String], widths: &[usize]) -> String {
    widths
        .iter()
        .enumerate()
        .map(|(index, width)| {
            // manual space padding: {:<width$} counts chars, not columns,
            // so it would under-pad multi-byte names.
            let value = row.get(index).map(String::as_str).unwrap_or("");
            let mut cell = String::from(value);
            cell.push_str(&" ".repeat(width.saturating_sub(display_width(value))));
            cell
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn render_separator(widths: &[usize]) -> String {
    widths
        .iter()
        .map(|width| "-".repeat(*width))
        .collect::<Vec<_>>()
        .join("  ")
}

#[cfg(test)]
#[path = "tests/output.rs"]
mod tests;
