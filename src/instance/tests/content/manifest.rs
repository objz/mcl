use super::*;

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
