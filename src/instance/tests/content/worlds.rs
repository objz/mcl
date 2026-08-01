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
    assert_eq!(entry.title_suffix.as_deref(), Some("Hardcore"));
    assert!(entry.description.contains("Last played:  2023-11-14 22:13"));
    assert!(
        entry
            .description
            .contains("Difficulty: Hard  •  Cheats: Off")
    );
    assert!(entry.description.contains("Minecraft:    1.21.1"));
    assert!(entry.description.contains("Approx. size:"));

    std::fs::write(world.join("level.dat"), b"not nbt").unwrap();
    let fallback = scan_one_world(&world, "world-folder", true);
    assert_eq!(fallback.name, "world-folder");
    assert_eq!(fallback.title_suffix, None);
    assert!(fallback.description.contains("Approx. size:"));
}
