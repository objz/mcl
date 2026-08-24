// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

// builds an in-memory zip in a tempdir with the given json as
// install_profile.json. lets the legacy-install-profile detector be
// tested without an actual forge installer.
fn make_installer_zip(tmp: &std::path::Path, json: &serde_json::Value) -> std::path::PathBuf {
    use std::io::Write;
    let path = tmp.join("installer.jar");
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let opts: zip::write::SimpleFileOptions = Default::default();
    zip.start_file("install_profile.json", opts).unwrap();
    zip.write_all(serde_json::to_string(json).unwrap().as_bytes())
        .unwrap();
    zip.finish().unwrap();
    path
}

#[test]
fn has_legacy_install_profile_true_when_version_info_present() {
    let tmp = tempfile::tempdir().unwrap();
    let jar = make_installer_zip(
        tmp.path(),
        &serde_json::json!({
            "install": {},
            "versionInfo": {
                "id": "1.7.10-Forge10.13.4.1614-1.7.10",
                "mainClass": "net.minecraft.launchwrapper.Launch"
            }
        }),
    );
    assert!(has_legacy_install_profile(&jar));
}

#[test]
fn has_legacy_install_profile_false_when_version_info_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let jar = make_installer_zip(
        tmp.path(),
        &serde_json::json!({
            "spec": 1,
            "minecraft": "1.20.1",
            "data": {}
        }),
    );
    assert!(!has_legacy_install_profile(&jar));
}

#[test]
fn has_legacy_install_profile_false_for_missing_jar() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(!has_legacy_install_profile(
        &tmp.path().join("missing-installer.jar")
    ));
}
