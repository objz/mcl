use std::io::Write;
use std::path::{Path, PathBuf};

use super::*;
use crate::net::modrinth::MrpackFile;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

#[test]
fn mod_files_download_to_their_manifest_paths() {
    let _guard = crate::tui::tests::UI_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/first.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"first".to_vec()))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/second.jar"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"second".to_vec()))
            .expect(1)
            .mount(&server)
            .await;
        let index = MrpackIndex {
            format_version: 1,
            game: "minecraft".to_owned(),
            version_id: "1".to_owned(),
            name: "Downloads".to_owned(),
            dependencies: Default::default(),
            files: vec![
                MrpackFile {
                    path: "mods/first.jar".to_owned(),
                    downloads: vec![format!("{}/first.jar", server.uri())],
                    file_size: 5,
                },
                MrpackFile {
                    path: "resourcepacks/second.jar".to_owned(),
                    downloads: vec![format!("{}/second.jar", server.uri())],
                    file_size: 6,
                },
            ],
        };
        let tmp = tempfile::tempdir().unwrap();

        download_mod_files(&index, tmp.path()).await.unwrap();

        assert_eq!(
            std::fs::read(tmp.path().join("mods/first.jar")).unwrap(),
            b"first"
        );
        assert_eq!(
            std::fs::read(tmp.path().join("resourcepacks/second.jar")).unwrap(),
            b"second"
        );
        crate::tui::progress::clear();
    });
}

#[test]
fn mod_file_without_a_download_url_fails_without_creating_a_file() {
    let _guard = crate::tui::tests::UI_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let index = MrpackIndex {
            format_version: 1,
            game: "minecraft".to_owned(),
            version_id: "1".to_owned(),
            name: "Missing URL".to_owned(),
            dependencies: Default::default(),
            files: vec![MrpackFile {
                path: "mods/missing.jar".to_owned(),
                downloads: Vec::new(),
                file_size: 0,
            }],
        };
        let tmp = tempfile::tempdir().unwrap();

        assert!(download_mod_files(&index, tmp.path()).await.is_err());
        assert!(!tmp.path().join("mods/missing.jar").exists());
        crate::tui::progress::clear();
    });
}
