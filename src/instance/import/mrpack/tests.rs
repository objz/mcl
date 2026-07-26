use std::io::Write;
use std::path::{Path, PathBuf};

use super::*;

const INDEX: &[u8] = br#"{
    "formatVersion": 1,
    "game": "minecraft",
    "versionId": "1.0.0",
    "name": "Test Pack",
    "dependencies": {
        "minecraft": "1.21.1",
        "fabric-loader": "0.16.14"
    },
    "files": []
}"#;

fn make_mrpack(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
    let path = dir.join("test.mrpack");
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = Default::default();
    zip.start_file("modrinth.index.json", options).unwrap();
    zip.write_all(INDEX).unwrap();
    for (name, contents) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(contents).unwrap();
    }
    zip.finish().unwrap();
    path
}

#[test]
fn summary_counts_both_override_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let path = make_mrpack(
        tmp.path(),
        &[
            ("overrides/config/test.toml", b"config"),
            ("client-overrides/options.txt", b"options"),
        ],
    );

    let summary = build_summary(&path).unwrap();

    assert_eq!(summary.name, "Test Pack");
    assert_eq!(summary.game_version, "1.21.1");
    assert_eq!(summary.loader, ModLoader::Fabric);
    assert_eq!(summary.loader_version.as_deref(), Some("0.16.14"));
    assert_eq!(summary.override_count, 2);
}

#[test]
fn extraction_merges_both_override_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let path = make_mrpack(
        tmp.path(),
        &[
            ("overrides/config/test.toml", b"config"),
            ("client-overrides/options.txt", b"options"),
        ],
    );
    let minecraft = tmp.path().join("minecraft");

    extract_overrides(&path, &minecraft).unwrap();

    assert_eq!(
        std::fs::read(minecraft.join("config/test.toml")).unwrap(),
        b"config"
    );
    assert_eq!(
        std::fs::read(minecraft.join("options.txt")).unwrap(),
        b"options"
    );
}

#[test]
fn extraction_rejects_path_traversal() {
    let tmp = tempfile::tempdir().unwrap();
    let path = make_mrpack(tmp.path(), &[("overrides/../../escaped.txt", b"escaped")]);
    let minecraft = tmp.path().join("minecraft");

    let error = extract_overrides(&path, &minecraft).unwrap_err();

    assert!(error.to_string().contains("Unsafe override path"));
    assert!(!tmp.path().join("escaped.txt").exists());
}
