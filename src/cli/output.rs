// plain-text table rendering for CLI output. no fancy box-drawing,
// just left-aligned columns with two-space gaps. keeps it pipeable.
use chrono::{DateTime, Utc};

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

// find the widest value in each column to pad everything evenly
fn column_widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = headers.iter().map(|header| header.len()).collect();

    for row in rows {
        if row.len() > widths.len() {
            widths.resize(row.len(), 0);
        }

        for (index, value) in row.iter().enumerate() {
            widths[index] = widths[index].max(value.len());
        }
    }

    widths
}

fn render_row(row: &[String], widths: &[usize]) -> String {
    widths
        .iter()
        .enumerate()
        .map(|(index, width)| {
            format!(
                "{:<width$}",
                row.get(index).map(String::as_str).unwrap_or(""),
                width = width
            )
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
