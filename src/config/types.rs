use dirs_next;
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct General {
    #[serde(default)]
    pub debug: bool,
}

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
    "~/.local/share/mcl/instances".to_string()
}

fn default_meta_dir() -> String {
    "~/.local/share/mcl/meta".to_string()
}

impl Default for Paths {
    fn default() -> Self {
        Paths {
            instances_dir: default_instances_dir(),
            meta_dir: default_meta_dir(),
            java_path: None,
        }
    }
}

impl Paths {
    pub fn effective_java_path(&self) -> Option<&str> {
        self.java_path.as_deref().filter(|s| !s.is_empty())
    }

    pub fn resolve_instances_dir(&self) -> std::path::PathBuf {
        let raw = &self.instances_dir;
        if let Some(stripped) = raw.strip_prefix("~/") {
            return match dirs_next::home_dir() {
                Some(home) => home.join(stripped),
                None => std::path::PathBuf::from(raw),
            };
        }
        if raw == "~" {
            return match dirs_next::home_dir() {
                Some(home) => home,
                None => std::path::PathBuf::from(raw),
            };
        }
        std::path::PathBuf::from(raw)
    }

    pub fn resolve_meta_dir(&self) -> std::path::PathBuf {
        let raw = &self.meta_dir;
        if let Some(stripped) = raw.strip_prefix("~/") {
            return match dirs_next::home_dir() {
                Some(home) => home.join(stripped),
                None => std::path::PathBuf::from(raw),
            };
        }
        if raw == "~" {
            return match dirs_next::home_dir() {
                Some(home) => home,
                None => std::path::PathBuf::from(raw),
            };
        }
        std::path::PathBuf::from(raw)
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
    "512M".to_string()
}

fn default_memory_max() -> String {
    "2G".to_string()
}

impl Default for Defaults {
    fn default() -> Self {
        Defaults {
            memory_min: default_memory_min(),
            memory_max: default_memory_max(),
        }
    }
}

#[derive(Debug, Deserialize)]
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
        Ui {
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
            java_path: Some("/usr/bin/java".to_string()),
            ..Paths::default()
        };
        assert_eq!(paths.effective_java_path(), Some("/usr/bin/java"));
    }

    #[test]
    fn resolve_instances_dir_absolute_path() {
        let paths = Paths {
            instances_dir: "/opt/mcl/instances".to_string(),
            ..Paths::default()
        };
        assert_eq!(
            paths.resolve_instances_dir(),
            std::path::PathBuf::from("/opt/mcl/instances")
        );
    }

    #[test]
    fn resolve_instances_dir_tilde_prefix() {
        let paths = Paths {
            instances_dir: "~/games/mcl".to_string(),
            ..Paths::default()
        };
        let resolved = paths.resolve_instances_dir();
        assert!(!resolved.to_string_lossy().starts_with('~'));
        assert!(resolved.to_string_lossy().ends_with("games/mcl"));
    }

    #[test]
    fn resolve_instances_dir_bare_tilde() {
        let paths = Paths {
            instances_dir: "~".to_string(),
            ..Paths::default()
        };
        let resolved = paths.resolve_instances_dir();
        assert!(!resolved.to_string_lossy().starts_with('~'));
    }

    #[test]
    fn resolve_meta_dir_absolute_path() {
        let paths = Paths {
            meta_dir: "/opt/mcl/meta".to_string(),
            ..Paths::default()
        };
        assert_eq!(
            paths.resolve_meta_dir(),
            std::path::PathBuf::from("/opt/mcl/meta")
        );
    }

    #[test]
    fn resolve_meta_dir_tilde_prefix() {
        let paths = Paths {
            meta_dir: "~/mcl/meta".to_string(),
            ..Paths::default()
        };
        let resolved = paths.resolve_meta_dir();
        assert!(!resolved.to_string_lossy().starts_with('~'));
        assert!(resolved.to_string_lossy().ends_with("mcl/meta"));
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
        assert!(!config.general.debug);
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
        assert!(config.general.debug);
        assert_eq!(config.defaults.memory_max, "8G");
        assert_eq!(config.defaults.memory_min, "512M");
    }
}
