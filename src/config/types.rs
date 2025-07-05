use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct General {
    #[serde(default)]
    pub debug: bool,
}

impl Default for General {
    fn default() -> Self {
        General { debug: false }
    }
}

#[derive(Debug, Deserialize)]
pub struct Paths {
    #[serde(default = "default_instances_dir")]
    pub instances_dir: String,
    pub java_path: Option<String>,
}

fn default_instances_dir() -> String {
    "~/.local/share/mcl/instances".to_string()
}

impl Default for Paths {
    fn default() -> Self {
        Paths {
            instances_dir: default_instances_dir(),
            java_path: None,
        }
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
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub paths: Paths,
    #[serde(default)]
    pub defaults: Defaults,
}
