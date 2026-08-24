// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use std::io::Write;

use flate2::{Compression, write::GzEncoder};

use super::*;

#[test]
fn world_scan_uses_level_dat_metadata_and_falls_back_cleanly() {
    let temp = tempfile::tempdir().unwrap();
    let world = temp.path().join("world-folder");
    std::fs::create_dir_all(world.join("region")).unwrap();
    std::fs::write(world.join("region/r.0.0.mca"), vec![0; 2048]).unwrap();

    let nbt = fastnbt::nbt!({
        "Data": {
            "LevelName": "Display World",
            "GameType": 0,
            "hardcore": 1_i8,
            "Difficulty": 3_i8,
            "allowCommands": 0_i8,
            "LastPlayed": 1_700_000_000_000_i64,
            "Version": { "Name": "1.21.1" },
            "DataVersion": 3955,
        }
    });
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&fastnbt::to_bytes(&nbt).unwrap())
        .unwrap();
    std::fs::write(world.join("level.dat"), encoder.finish().unwrap()).unwrap();

    let entry = scan_one_world(&world, "world-folder", true);
    assert_eq!(entry.name, "Display World");
    let details = entry.world_details.as_ref().unwrap();
    assert_eq!(details.game_mode, Some(WorldGameMode::Hardcore));
    assert_eq!(
        details.last_played,
        chrono::DateTime::from_timestamp(1_700_000_000, 0)
    );
    assert_eq!(details.minecraft_version.as_deref(), Some("1.21.1"));
    assert_eq!(details.size.as_deref(), Some("2.2 KB"));
    assert!(entry.description.is_empty());

    std::fs::write(world.join("level.dat"), b"not nbt").unwrap();
    let fallback = scan_one_world(&world, "world-folder", true);
    assert_eq!(fallback.name, "world-folder");
    assert_eq!(fallback.title_suffix, None);
    assert_eq!(
        fallback
            .world_details
            .as_ref()
            .and_then(|details| details.size.as_deref()),
        Some("2.0 KB")
    );
}

#[test]
fn world_scan_lists_directory_and_zip_datapacks() {
    let temp = tempfile::tempdir().unwrap();
    let world = temp.path().join("world");
    std::fs::create_dir_all(world.join("datapacks/folder-pack")).unwrap();
    std::fs::write(world.join("datapacks/zipped-pack.zip"), b"zip").unwrap();
    std::fs::write(world.join("datapacks/disabled-pack.zip.disabled"), b"zip").unwrap();

    let details = scan_one_world(&world, "world", true).world_details.unwrap();

    assert_eq!(
        details.datapacks,
        ["disabled-pack", "folder-pack", "zipped-pack"]
    );
}
