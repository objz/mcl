use super::*;

#[test]
fn effective_java_path_none_when_absent() {
    let paths = Paths {
        java_path: None,
        ..Paths::default()
    };
    assert!(paths.effective_java_path().is_none());
}

#[test]
fn effective_java_path_none_when_empty() {
    let paths = Paths {
        java_path: Some(String::new()),
        ..Paths::default()
    };
    assert!(paths.effective_java_path().is_none());
}

#[test]
fn effective_java_path_some_when_set() {
    let paths = Paths {
        java_path: Some("/usr/bin/java".to_owned()),
        ..Paths::default()
    };
    assert_eq!(paths.effective_java_path(), Some("/usr/bin/java"));
}

#[test]
fn blank_curseforge_key_keeps_provider_disabled() {
    let content: Content = toml::from_str("curseforge_api_key = \"  \"").unwrap();
    assert_eq!(content.curseforge_api_key(), None);
    assert_eq!(content.preferred_provider(), "modrinth");
}

#[test]
fn curseforge_preference_requires_its_api_key() {
    let without_key: Content = toml::from_str("preferred_provider = \"curseforge\"").unwrap();
    assert_eq!(without_key.preferred_provider(), "modrinth");

    let configured: Content =
        toml::from_str("preferred_provider = \"curseforge\"\ncurseforge_api_key = \"secret\"")
            .unwrap();
    assert_eq!(configured.preferred_provider(), "curseforge");
}

#[test]
fn resolve_path_absolute() {
    assert_eq!(resolve_path("/opt/rmcl"), PathBuf::from("/opt/rmcl"));
}

#[test]
fn resolve_path_tilde_prefix() {
    let resolved = resolve_path("~/games/rmcl");
    assert!(!resolved.to_string_lossy().starts_with('~'));
    assert!(resolved.to_string_lossy().ends_with("games/rmcl"));
}

#[test]
fn resolve_path_bare_tilde() {
    let resolved = resolve_path("~");
    assert!(!resolved.to_string_lossy().starts_with('~'));
}

#[test]
fn config_deserializes_from_empty_toml() {
    let config: Config = toml::from_str("").unwrap();
    assert_eq!(config.defaults.memory_max, "2G");
}

#[test]
fn config_deserializes_partial_toml() {
    let toml_str = r#"
[general]
debug = true

[defaults]
memory_max = "8G"
"#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.defaults.memory_max, "8G");
    assert_eq!(config.defaults.memory_min, "512M");
}
