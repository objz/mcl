// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn load_config_from_valid_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
            [defaults]
            memory_max = "4G"
            "#,
    )
    .unwrap();
    let config = load_config(&path).unwrap();
    assert_eq!(config.defaults.memory_max, "4G");
    assert_eq!(config.defaults.memory_min, "512M");
}

#[test]
fn load_config_from_empty_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    std::fs::write(&path, "").unwrap();
    let config = load_config(&path).unwrap();
    assert_eq!(config.defaults.memory_max, "2G");
}

#[test]
fn load_config_missing_file_uses_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("nonexistent.toml");
    let config = load_config(&path).unwrap();
    assert_eq!(config.defaults.memory_max, "2G");
    assert_eq!(config.defaults.memory_min, "512M");
}

#[test]
fn load_config_partial_sections() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    std::fs::write(
        &path,
        r#"
            [paths]
            instances_dir = "/custom/path"
            "#,
    )
    .unwrap();
    let config = load_config(&path).unwrap();
    assert_eq!(config.paths.instances_dir, "/custom/path");
    assert!(config.paths.java_path.is_none());
}
