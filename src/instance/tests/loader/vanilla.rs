// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::net::mojang::{LatestVersions, VersionEntry, VersionManifest};

#[test]
fn manifest_types_map_to_stable_game_versions() {
    let manifest = VersionManifest {
        latest: LatestVersions {
            release: "1.21.1".to_owned(),
            snapshot: "24w01a".to_owned(),
        },
        versions: vec![
            VersionEntry {
                id: "1.21.1".to_owned(),
                version_type: "release".to_owned(),
                url: String::new(),
                sha1: String::new(),
            },
            VersionEntry {
                id: "24w01a".to_owned(),
                version_type: "snapshot".to_owned(),
                url: String::new(),
                sha1: String::new(),
            },
        ],
    };

    let versions = game_versions_from_manifest(manifest);

    assert_eq!(versions[0].id, "1.21.1");
    assert!(versions[0].stable);
    assert_eq!(versions[1].id, "24w01a");
    assert!(!versions[1].stable);
}
