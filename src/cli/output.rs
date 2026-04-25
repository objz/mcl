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
mod tests {
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
    fn formats_datetime_consistently() {
        let dt = Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
        assert_eq!(format_datetime(&dt), "2024-01-02 03:04:05 UTC");
    }
}
