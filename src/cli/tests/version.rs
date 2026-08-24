// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::filter_manifest_versions;
use crate::net::mojang::{LatestVersions, VersionEntry, VersionManifest};
use std::collections::HashSet;

fn manifest() -> VersionManifest {
    VersionManifest {
        latest: LatestVersions {
            release: "1.20.1".to_string(),
            snapshot: "24w01a".to_string(),
        },
        versions: vec![
            VersionEntry {
                id: "1.20.1".to_string(),
                version_type: "release".to_string(),
                url: "https://example.com/release".to_string(),
                sha1: "a".to_string(),
            },
            VersionEntry {
                id: "24w01a".to_string(),
                version_type: "snapshot".to_string(),
                url: "https://example.com/snapshot".to_string(),
                sha1: "b".to_string(),
            },
        ],
    }
}

#[test]
fn filters_out_snapshots_by_default() {
    let rows = filter_manifest_versions(&manifest(), None, false);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "1.20.1");
}

#[test]
fn intersects_supported_versions() {
    let supported = HashSet::from(["24w01a".to_string()]);
    let rows = filter_manifest_versions(&manifest(), Some(&supported), true);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "24w01a");
}
