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

#[test]
fn bundled_config_uses_platform_paths_and_automatic_java() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    std::fs::write(&path, include_str!("../../../assets/config.toml")).unwrap();
    let config = load_config(&path).unwrap();
    assert_eq!(
        config.paths.instances_dir,
        settings::Paths::default().instances_dir
    );
    assert_eq!(config.paths.meta_dir, settings::Paths::default().meta_dir);
    assert!(config.paths.java_path.is_none());
    assert_eq!(config.ui.image_protocol, settings::ImageProtocol::Auto);
}

#[test]
fn config_normalizes_memory_java_and_notification_bounds() {
    let config = Config {
        paths: settings::Paths {
            instances_dir: String::new(),
            meta_dir: "  ".to_owned(),
            java_path: Some("  ".to_owned()),
        },
        defaults: settings::Defaults {
            memory_min: "invalid".to_owned(),
            memory_max: "1G".to_owned(),
            ..Default::default()
        },
        ui: settings::Ui {
            error_auto_dismiss_ms: 100,
            error_slide_start_ms: 200,
            error_fly_out_ms: 300,
            max_error_events: 0,
            ..Default::default()
        },
        ..Default::default()
    }
    .normalize();
    assert_eq!(
        config.paths.instances_dir,
        settings::Paths::default().instances_dir
    );
    assert_eq!(config.paths.meta_dir, settings::Paths::default().meta_dir);
    assert!(config.paths.java_path.is_none());
    assert_eq!(config.defaults.memory_min, "512M");
    assert_eq!(config.defaults.memory_max, "1G");
    assert_eq!(config.ui.error_slide_start_ms, 100);
    assert_eq!(config.ui.error_fly_out_ms, 100);
    assert_eq!(config.ui.max_error_events, 1);
}

#[test]
fn settings_writer_preserves_comments_and_unknown_keys() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("config.toml");
    std::fs::write(
        &path,
        "# keep this comment\n[defaults]\n# heap comment\nmemory_max = \"2G\"\n\n[future]\nvalue = 42\n",
    )
    .unwrap();
    let mut config = Config::default();
    config.defaults.memory_max = "8G".to_owned();
    write_config_document(&path, &config).unwrap();
    let saved = std::fs::read_to_string(path).unwrap();
    assert!(saved.contains("# keep this comment"), "{saved}");
    assert!(saved.contains("# heap comment"), "{saved}");
    assert!(saved.contains("memory_max = \"8G\""));
    assert!(saved.contains("[future]"));
    assert!(saved.contains("value = 42"));
    assert!(!saved.contains("instances_dir"));
    assert!(!saved.contains("meta_dir"));
}
