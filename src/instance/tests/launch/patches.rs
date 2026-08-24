// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use std::io::Write;

// builds an in-memory zip with the given entries, writes it to tmp, and
// returns the path. shared by parse_add_opens tests.
fn make_zip(tmp: &Path, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
    let path = tmp.join(name);
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions = Default::default();
    for (filename, bytes) in entries {
        zip.start_file(*filename, opts).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    path
}

#[test]
fn strip_replaced_libs_removes_dominated_prefixes() {
    let mut classpath = vec![
        PathBuf::from("/libs/launchwrapper-1.12.jar"),
        PathBuf::from("/libs/asm-all-5.0.3.jar"),
        PathBuf::from("/libs/lwjgl-2.9.4.jar"),
        PathBuf::from("/libs/lwjgl_util-2.9.4.jar"),
        PathBuf::from("/libs/commons-compress-1.4.1.jar"),
        PathBuf::from("/libs/commons-io-2.4.jar"),
        PathBuf::from("/libs/guava-15.0.jar"),
        // these stay
        PathBuf::from("/libs/log4j-core-2.0.jar"),
        PathBuf::from("/libs/guava-21.0.jar"),
    ];
    strip_replaced_libs(&mut classpath);
    assert_eq!(
        classpath,
        vec![
            PathBuf::from("/libs/log4j-core-2.0.jar"),
            PathBuf::from("/libs/guava-21.0.jar"),
        ]
    );
}

#[test]
fn strip_replaced_libs_keeps_unrelated_entries() {
    let mut classpath = vec![
        PathBuf::from("/libs/log4j-core-2.0.jar"),
        PathBuf::from("/libs/mixin-0.8.5.jar"),
    ];
    let original = classpath.clone();
    strip_replaced_libs(&mut classpath);
    assert_eq!(classpath, original);
}

#[test]
fn parse_add_opens_extracts_module_args() {
    let tmp = tempfile::tempdir().unwrap();
    let manifest = b"Manifest-Version: 1.0\nAdd-Opens: java.base/java.lang java.base/java.util\n";
    let zip_path = make_zip(
        tmp.path(),
        "patches.zip",
        &[("META-INF/MANIFEST.MF", manifest)],
    );
    let args = parse_add_opens(&zip_path).expect("parsed");
    assert_eq!(
        args,
        vec![
            "--add-opens",
            "java.base/java.lang=ALL-UNNAMED",
            "--add-opens",
            "java.base/java.util=ALL-UNNAMED",
        ]
    );
}

#[test]
fn parse_add_opens_handles_continuation_lines() {
    // jar manifests wrap long lines: the line ends with a trailing space,
    // then the next physical line starts with a leading space marker.
    // when joined per the MANIFEST.MF spec, the trailing space remains
    // and the leading space marker is consumed, so the two values stay
    // separated by exactly one space when split_whitespace runs.
    let tmp = tempfile::tempdir().unwrap();
    let manifest =
        b"Manifest-Version: 1.0\nAdd-Opens: java.base/java.lang \n java.base/sun.security.util\n";
    let zip_path = make_zip(
        tmp.path(),
        "patches-continuation.zip",
        &[("META-INF/MANIFEST.MF", manifest)],
    );
    let args = parse_add_opens(&zip_path).expect("parsed");
    assert_eq!(
        args,
        vec![
            "--add-opens",
            "java.base/java.lang=ALL-UNNAMED",
            "--add-opens",
            "java.base/sun.security.util=ALL-UNNAMED",
        ]
    );
}

#[test]
fn parse_add_opens_returns_none_when_manifest_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let zip_path = make_zip(tmp.path(), "no-manifest.zip", &[("other.txt", b"x")]);
    assert!(parse_add_opens(&zip_path).is_none());
}

#[test]
fn parse_add_opens_returns_none_for_missing_file() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(parse_add_opens(&tmp.path().join("missing.zip")).is_none());
}

#[test]
fn add_lwjgl3_inserts_only_jars_that_exist() {
    let tmp = tempfile::tempdir().unwrap();
    let lib_dir = tmp.path();

    // pre-create only the lwjgl core jar; everything else absent so the
    // function should skip them silently. proves we don't insert paths
    // for jars that aren't there.
    let core_jar = lib_dir.join("org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3.jar");
    std::fs::create_dir_all(core_jar.parent().unwrap()).unwrap();
    std::fs::write(&core_jar, b"jar").unwrap();

    let mut classpath = vec![PathBuf::from("/leading/forge-patches.jar")];
    add_lwjgl3(lib_dir, &mut classpath);

    // forge-patches stays at index 0; lwjgl core is inserted at index 1.
    // none of the other modules existed so nothing else was added.
    assert_eq!(classpath[0], PathBuf::from("/leading/forge-patches.jar"));
    assert_eq!(classpath[1], core_jar);
    assert_eq!(classpath.len(), 2);
}
