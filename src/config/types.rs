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
