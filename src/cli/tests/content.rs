// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::find_entry_by_stem;
use crate::instance::content::entry::ContentEntry;
use std::path::PathBuf;

fn entry(file_stem: &str) -> ContentEntry {
    ContentEntry {
        file_stem: file_stem.to_string(),
        name: file_stem.to_string(),
        source_slug: None,
        installed_path: None,
        provider_project: None,
        world_details: None,
        title_suffix: None,
        footer_label: None,
        footer_change: None,
        description: String::new(),
        enabled: true,
        icon_bytes: None,
        provider_icon: false,
        provider_description: false,
        path: PathBuf::from(file_stem),
        icon_lines: None,
    }
}

#[test]
fn matches_by_stem_case_insensitively() {
    let entries = vec![entry("Sodium"), entry("Lithium")];
    let found = find_entry_by_stem(&entries, "sOdIuM").expect("entry should match");
    assert_eq!(found.file_stem, "Sodium");
}

#[test]
fn returns_none_for_missing_stem() {
    let entries = vec![entry("Sodium")];
    assert!(find_entry_by_stem(&entries, "iris").is_none());
}
