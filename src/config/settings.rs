// all the config structs that map to sections in config.toml.
// everything has sane defaults so a blank file (or no file) still works.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct General {}

#[derive(Debug, Deserialize)]
pub struct Paths {
    #[serde(default = "default_instances_dir")]
    pub instances_dir: String,
    #[serde(default = "default_meta_dir")]
    pub meta_dir: String,
    #[serde(default)]
    pub java_path: Option<String>,
}

fn default_instances_dir() -> String {
    dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mcl")
        .join("instances")
        .to_string_lossy()
        .into_owned()
}

fn default_meta_dir() -> String {
    dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mcl")
        .join("meta")
        .to_string_lossy()
        .into_owned()
}

impl Default for Paths {
    fn default() -> Self {
        Self {
            instances_dir: default_instances_dir(),
            meta_dir: default_meta_dir(),
            java_path: None,
        }
    }
}

// expand ~ in paths since toml doesn't do that for us
pub fn resolve_path(raw: &str) -> PathBuf {
    if let Some(stripped) = raw.strip_prefix("~/") {
        if let Some(home) = dirs_next::home_dir() {
            return home.join(stripped);
        }
    } else if raw == "~"
        && let Some(home) = dirs_next::home_dir() {
            return home;
        }
    PathBuf::from(raw)
}

impl Paths {
    pub fn effective_java_path(&self) -> Option<&str> {
        self.java_path.as_deref().filter(|s| !s.is_empty())
    }

    pub fn resolve_instances_dir(&self) -> PathBuf {
        resolve_path(&self.instances_dir)
    }

    pub fn resolve_meta_dir(&self) -> PathBuf {
        resolve_path(&self.meta_dir)
    }
}

#[derive(Debug, Deserialize)]
pub struct Defaults {
    #[serde(default = "default_memory_min")]
    pub memory_min: String,
    #[serde(default = "default_memory_max")]
    pub memory_max: String,
}

fn default_memory_min() -> String {
    "512M".to_owned()
}
fn default_memory_max() -> String {
    "2G".to_owned()
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            memory_min: default_memory_min(),
            memory_max: default_memory_max(),
        }
    }
}

#[derive(Debug, Deserialize)]
// timing knobs for the error toast animation: show for 5s, start sliding at 3.5s,
// fly off screen over 300ms. tweak these if the toasts feel too fast or slow.
pub struct Ui {
    #[serde(default = "default_error_auto_dismiss_ms")]
    pub error_auto_dismiss_ms: u64,
    #[serde(default = "default_error_slide_start_ms")]
    pub error_slide_start_ms: u64,
    #[serde(default = "default_error_fly_out_ms")]
    pub error_fly_out_ms: u64,
    #[serde(default = "default_max_error_events")]
    pub max_error_events: usize,
}

fn default_error_auto_dismiss_ms() -> u64 {
    5000
}
fn default_error_slide_start_ms() -> u64 {
    3500
}
fn default_error_fly_out_ms() -> u64 {
    300
}
fn default_max_error_events() -> usize {
    50
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            error_auto_dismiss_ms: default_error_auto_dismiss_ms(),
            error_slide_start_ms: default_error_slide_start_ms(),
            error_fly_out_ms: default_error_fly_out_ms(),
            max_error_events: default_max_error_events(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub paths: Paths,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub ui: Ui,
}

#[cfg(test)]
mod tests {
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
    fn resolve_path_absolute() {
        assert_eq!(resolve_path("/opt/mcl"), PathBuf::from("/opt/mcl"));
    }

    #[test]
    fn resolve_path_tilde_prefix() {
        let resolved = resolve_path("~/games/mcl");
        assert!(!resolved.to_string_lossy().starts_with('~'));
        assert!(resolved.to_string_lossy().ends_with("games/mcl"));
    }

    #[test]
    fn resolve_path_bare_tilde() {
        let resolved = resolve_path("~");
        assert!(!resolved.to_string_lossy().starts_with('~'));
    }

    #[test]
    fn resolve_instances_dir_absolute() {
        let paths = Paths {
            instances_dir: "/opt/mcl/instances".to_owned(),
            ..Paths::default()
        };
        assert_eq!(
            paths.resolve_instances_dir(),
            PathBuf::from("/opt/mcl/instances")
        );
    }

    #[test]
    fn resolve_meta_dir_absolute() {
        let paths = Paths {
            meta_dir: "/opt/mcl/meta".to_owned(),
            ..Paths::default()
        };
        assert_eq!(paths.resolve_meta_dir(), PathBuf::from("/opt/mcl/meta"));
    }

    #[test]
    fn defaults_have_expected_values() {
        let d = Defaults::default();
        assert_eq!(d.memory_min, "512M");
        assert_eq!(d.memory_max, "2G");
    }

    #[test]
    fn ui_defaults_have_expected_values() {
        let ui = Ui::default();
        assert_eq!(ui.error_auto_dismiss_ms, 5000);
        assert_eq!(ui.error_slide_start_ms, 3500);
        assert_eq!(ui.error_fly_out_ms, 300);
        assert_eq!(ui.max_error_events, 50);
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
}
