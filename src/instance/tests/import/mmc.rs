// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn parse_mmc_pack_json() {
    let json = r#"{
            "formatVersion": 1,
            "components": [
                {
                    "uid": "net.minecraft",
                    "version": "1.7.10",
                    "cachedName": "Minecraft"
                },
                {
                    "uid": "net.minecraftforge",
                    "version": "10.13.4.1614",
                    "cachedName": "Forge"
                }
            ]
        }"#;
    let pack: MmcPack = serde_json::from_str(json).unwrap();
    assert_eq!(pack.game_version(), Some("1.7.10".to_string()));
    let (loader, version) = pack.loader();
    assert_eq!(loader, Some(ModLoader::Forge));
    assert_eq!(version, Some("10.13.4.1614".to_string()));
}

#[test]
fn parse_mmc_pack_vanilla() {
    let json = r#"{
            "formatVersion": 1,
            "components": [
                {"uid": "net.minecraft", "version": "1.21.4"}
            ]
        }"#;
    let pack: MmcPack = serde_json::from_str(json).unwrap();
    assert!(pack.loader().0.is_none());
}

// builds an in-memory mmc-style pack zip and verifies that
// extract_mmc_archive copies only the .minecraft/ subtree into the
// destination, preserving relative paths and skipping siblings.
#[test]
fn extract_mmc_archive_copies_minecraft_subtree() {
    use std::io::Write;

    let tmp = tempfile::tempdir().unwrap();
    let archive_path = tmp.path().join("pack.zip");
    let dest = tmp.path().join("instance/.minecraft");
    std::fs::create_dir_all(&dest).unwrap();

    // Pack/ is the prefix; only .minecraft/ entries should land in dest.
    // mmc-style pack: a root dir "Pack/" wrapping the .minecraft tree
    // plus a sibling mmc-pack.json that should NOT be extracted.
    {
        let file = std::fs::File::create(&archive_path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let opts: zip::write::SimpleFileOptions = Default::default();

        zip.start_file("Pack/mmc-pack.json", opts).unwrap();
        zip.write_all(b"{}").unwrap();

        zip.start_file("Pack/.minecraft/options.txt", opts).unwrap();
        zip.write_all(b"lang:en_us").unwrap();

        zip.start_file("Pack/.minecraft/mods/test-mod.jar", opts)
            .unwrap();
        zip.write_all(b"jar-bytes").unwrap();

        zip.finish().unwrap();
    }

    extract_mmc_archive(&archive_path, &dest).expect("extract");

    // .minecraft/ entries must have been copied with their relative paths
    let options = std::fs::read(dest.join("options.txt")).expect("options.txt");
    assert_eq!(options, b"lang:en_us");
    let modjar = std::fs::read(dest.join("mods/test-mod.jar")).expect("mods/test-mod.jar");
    assert_eq!(modjar, b"jar-bytes");

    // and the sibling outside .minecraft/ must not have been copied
    assert!(
        !dest.join("mmc-pack.json").exists(),
        "mmc-pack.json should not land in the instance dir"
    );
}

#[test]
fn extract_mmc_archive_rejects_path_traversal() {
    use std::io::Write;

    let tmp = tempfile::tempdir().unwrap();
    let archive_path = tmp.path().join("pack.zip");
    let dest = tmp.path().join("instance/minecraft");
    let file = std::fs::File::create(&archive_path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = Default::default();
    zip.start_file("Pack/mmc-pack.json", options).unwrap();
    zip.write_all(b"{}").unwrap();
    zip.start_file("Pack/.minecraft/../../escaped.txt", options)
        .unwrap();
    zip.write_all(b"escaped").unwrap();
    zip.finish().unwrap();

    let error = extract_mmc_archive(&archive_path, &dest).unwrap_err();

    assert!(error.to_string().contains("archive path"));
    assert!(!tmp.path().join("escaped.txt").exists());
}
