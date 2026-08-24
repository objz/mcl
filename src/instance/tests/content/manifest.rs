// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn curseforge_fingerprint_ignores_whitespace() {
    let temp = tempfile::tempdir().unwrap();
    let compact = temp.path().join("compact.jar");
    let spaced = temp.path().join("spaced.jar");
    std::fs::write(&compact, b"abc").unwrap();
    std::fs::write(&spaced, b"a b\nc\r\t").unwrap();
    let compact = fingerprint(&compact).unwrap();
    assert_eq!(compact.hash("curseforge"), Some("1621425345"));
    assert_eq!(
        compact.hash("curseforge"),
        fingerprint(&spaced).unwrap().hash("curseforge")
    );
}

#[test]
fn manifest_round_trip_and_lookup_are_exact() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("rmcl/content/manifest.json");
    let record = ContentFileRecord {
        relative_path: PathBuf::from("mods/fabric-api.jar"),
        kind: ContentKind::Mod,
        enabled: true,
        fingerprint: FileFingerprint {
            size: 3,
            modified_ns: 4,
            hashes: BTreeMap::from([("sha1".to_owned(), "abc".to_owned())]),
        },
        resolution: Resolution::Resolved {
            project: ProviderProject {
                provider: "modrinth".to_owned(),
                project_id: "P7dR8mSH".to_owned(),
                version_id: "version".to_owned(),
            },
        },
        provider_aliases: Vec::new(),
        provider_checks: Vec::new(),
        required_dependencies: Vec::new(),
        automatic_dependency: false,
        cleanup_eligible: false,
    };
    let mut manifest = ContentManifest::default();
    manifest.upsert(record);
    manifest.save(&path).unwrap();

    let loaded = ContentManifest::load(&path).unwrap();
    assert_eq!(
        loaded.resolved_project_path("modrinth", "P7dR8mSH", Path::new("/instance/minecraft")),
        Some(PathBuf::from("/instance/minecraft/mods/fabric-api.jar"))
    );
    assert!(
        loaded
            .resolved_project_path("modrinth", "fabric-api", Path::new("/minecraft"))
            .is_none()
    );
}

#[test]
fn fingerprint_contains_both_modrinth_hashes() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("example.jar");
    std::fs::write(&path, b"abc").unwrap();
    let fingerprint = fingerprint(&path).unwrap();
    assert_eq!(
        fingerprint.hash("sha1"),
        Some("a9993e364706816aba3e25717850c26c9cd0d89d")
    );
    assert_eq!(fingerprint.hash("sha512").unwrap().len(), 128);
}

#[test]
fn renaming_a_record_preserves_its_resolution() {
    let mut manifest = ContentManifest::default();
    manifest.upsert(ContentFileRecord {
        relative_path: PathBuf::from("mods/example.jar"),
        kind: ContentKind::Mod,
        enabled: true,
        fingerprint: FileFingerprint {
            size: 3,
            modified_ns: 4,
            hashes: BTreeMap::new(),
        },
        resolution: Resolution::Resolved {
            project: ProviderProject {
                provider: "modrinth".to_owned(),
                project_id: "project".to_owned(),
                version_id: "version".to_owned(),
            },
        },
        provider_aliases: Vec::new(),
        provider_checks: Vec::new(),
        required_dependencies: Vec::new(),
        automatic_dependency: false,
        cleanup_eligible: false,
    });

    assert!(manifest.rename_record(
        Path::new("mods/example.jar"),
        Path::new("mods/example.jar.disabled"),
        false,
    ));
    let record = manifest
        .record(Path::new("mods/example.jar.disabled"))
        .unwrap();
    assert!(!record.enabled);
    assert!(matches!(record.resolution, Resolution::Resolved { .. }));
}

fn managed_record(
    path: &str,
    project_id: &str,
    automatic_dependency: bool,
    required_dependencies: Vec<ProviderProject>,
) -> ContentFileRecord {
    ContentFileRecord {
        relative_path: PathBuf::from(path),
        kind: ContentKind::Mod,
        enabled: true,
        fingerprint: FileFingerprint {
            size: 1,
            modified_ns: 1,
            hashes: BTreeMap::new(),
        },
        resolution: Resolution::Resolved {
            project: ProviderProject {
                provider: "modrinth".to_owned(),
                project_id: project_id.to_owned(),
                version_id: format!("{project_id}-version"),
            },
        },
        provider_aliases: Vec::new(),
        provider_checks: Vec::new(),
        required_dependencies,
        automatic_dependency,
        cleanup_eligible: automatic_dependency,
    }
}

fn dependency(project_id: &str) -> ProviderProject {
    ProviderProject {
        provider: "modrinth".to_owned(),
        project_id: project_id.to_owned(),
        version_id: format!("{project_id}-version"),
    }
}

#[test]
fn provider_aliases_match_the_same_installed_record() {
    let mut record = managed_record("mods/library.jar", "curseforge-library", false, Vec::new());
    record.provider_aliases.push(ProviderProject {
        provider: "modrinth".to_owned(),
        project_id: "modrinth-library".to_owned(),
        version_id: "modrinth-version".to_owned(),
    });
    let manifest = ContentManifest {
        version: 1,
        files: vec![record],
    };

    assert!(
        manifest
            .resolved_project_record("modrinth", "modrinth-library")
            .is_some()
    );
}

#[test]
fn orphan_cleanup_keeps_shared_and_user_managed_libraries() {
    let manifest = ContentManifest {
        version: 1,
        files: vec![
            managed_record("mods/first.jar", "first", false, vec![dependency("shared")]),
            managed_record(
                "mods/second.jar",
                "second",
                false,
                vec![dependency("shared"), dependency("explicit")],
            ),
            managed_record("mods/shared.jar", "shared", true, Vec::new()),
            managed_record("mods/explicit.jar", "explicit", false, Vec::new()),
        ],
    };

    assert_eq!(
        manifest.orphaned_dependencies_after_removing(Path::new("mods/first.jar")),
        Vec::<PathBuf>::new()
    );
    assert_eq!(
        manifest.orphaned_dependencies_after_removing(Path::new("mods/second.jar")),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn orphan_cleanup_follows_automatic_dependency_chains() {
    let manifest = ContentManifest {
        version: 1,
        files: vec![
            managed_record("mods/root.jar", "root", false, vec![dependency("library")]),
            managed_record(
                "mods/library.jar",
                "library",
                true,
                vec![dependency("nested")],
            ),
            managed_record("mods/nested.jar", "nested", true, Vec::new()),
        ],
    };

    assert_eq!(
        manifest.orphaned_dependencies_after_removing(Path::new("mods/root.jar")),
        vec![
            PathBuf::from("mods/library.jar"),
            PathBuf::from("mods/nested.jar")
        ]
    );
}

#[test]
fn orphan_cleanup_keeps_automatic_non_library_dependencies() {
    let mut dependency_record = managed_record("mods/sodium.jar", "sodium", true, Vec::new());
    dependency_record.cleanup_eligible = false;
    let manifest = ContentManifest {
        version: 1,
        files: vec![
            managed_record("mods/root.jar", "root", false, vec![dependency("sodium")]),
            dependency_record,
        ],
    };

    assert!(
        manifest
            .orphaned_dependencies_after_removing(Path::new("mods/root.jar"))
            .is_empty()
    );
}

#[test]
fn datapack_dependencies_are_scoped_to_their_world() {
    let mut first_root = managed_record(
        "saves/first/datapacks/root.zip",
        "root",
        false,
        vec![dependency("library")],
    );
    first_root.kind = ContentKind::DataPack;
    let mut second_root = managed_record(
        "saves/second/datapacks/root.zip",
        "root",
        false,
        vec![dependency("library")],
    );
    second_root.kind = ContentKind::DataPack;
    let mut first_library = managed_record(
        "saves/first/datapacks/library.zip",
        "library",
        true,
        Vec::new(),
    );
    first_library.kind = ContentKind::DataPack;
    let mut second_library = managed_record(
        "saves/second/datapacks/library.zip",
        "library",
        true,
        Vec::new(),
    );
    second_library.kind = ContentKind::DataPack;
    let manifest = ContentManifest {
        version: 1,
        files: vec![first_root, second_root, first_library, second_library],
    };

    assert_eq!(
        manifest.orphaned_dependencies_after_removing(Path::new("saves/first/datapacks/root.zip")),
        vec![PathBuf::from("saves/first/datapacks/library.zip")]
    );
}
