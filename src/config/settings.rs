// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// all the config structs that map to sections in config.toml.
// everything has sane defaults so a blank file (or no file) still works.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ImageProtocol {
    Halfblocks,
    Quadrants,
    #[default]
    Kitty,
    Iterm2,
}

#[derive(Debug, Deserialize, Default)]
pub struct General {}

#[derive(Debug, Deserialize)]
pub struct Content {
    #[serde(default = "default_true")]
    pub ask_on_provider_conflict: bool,
    #[serde(default = "default_provider")]
    pub preferred_provider: String,
    #[serde(default)]
    pub preferred_provider_only: bool,
    #[serde(default = "default_unmatched_retry_hours")]
    pub unmatched_retry_hours: u64,
    #[serde(default = "default_max_fingerprint_size_mib")]
    pub max_fingerprint_size_mib: u64,
}

fn default_true() -> bool {
    true
}

fn default_provider() -> String {
    "modrinth".to_owned()
}

fn default_unmatched_retry_hours() -> u64 {
    24
}

fn default_max_fingerprint_size_mib() -> u64 {
    512
}

impl Default for Content {
    fn default() -> Self {
        Self {
            ask_on_provider_conflict: true,
            preferred_provider: default_provider(),
            preferred_provider_only: false,
            unmatched_retry_hours: default_unmatched_retry_hours(),
            max_fingerprint_size_mib: default_max_fingerprint_size_mib(),
        }
    }
}

impl Content {
    pub fn preferred_provider(&self) -> &str {
        self.preferred_provider_with_curseforge(crate::net::curseforge::api_key().is_some())
    }

    pub fn discovery_provider_enabled(&self, provider: &str) -> bool {
        self.discovery_provider_enabled_with_curseforge(
            provider,
            crate::net::curseforge::api_key().is_some(),
        )
    }

    pub fn discovery_provider_label(&self) -> &'static str {
        self.discovery_provider_label_with_curseforge(crate::net::curseforge::api_key().is_some())
    }

    fn preferred_provider_with_curseforge(&self, curseforge_available: bool) -> &'static str {
        if self.preferred_provider.eq_ignore_ascii_case("curseforge") && curseforge_available {
            "curseforge"
        } else {
            "modrinth"
        }
    }

    fn discovery_provider_enabled_with_curseforge(
        &self,
        provider: &str,
        curseforge_available: bool,
    ) -> bool {
        match provider {
            "modrinth" => {
                !self.preferred_provider_only
                    || self.preferred_provider_with_curseforge(curseforge_available) == "modrinth"
            }
            "curseforge" => {
                curseforge_available
                    && (!self.preferred_provider_only
                        || self.preferred_provider_with_curseforge(curseforge_available)
                            == "curseforge")
            }
            _ => false,
        }
    }

    fn discovery_provider_label_with_curseforge(&self, curseforge_available: bool) -> &'static str {
        match (
            self.discovery_provider_enabled_with_curseforge("modrinth", curseforge_available),
            self.discovery_provider_enabled_with_curseforge("curseforge", curseforge_available),
        ) {
            (true, true) => "providers",
            (false, true) => "CurseForge",
            _ => "Modrinth",
        }
    }
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
    dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rmcl")
        .join("instances")
        .to_string_lossy()
        .into_owned()
}

fn default_meta_dir() -> String {
    dirs_next::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rmcl")
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
        && let Some(home) = dirs_next::home_dir()
    {
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
    #[serde(default)]
    pub image_protocol: ImageProtocol,
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
            image_protocol: ImageProtocol::default(),
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
    #[serde(default)]
    pub content: Content,
}

#[cfg(test)]
#[path = "tests/settings.rs"]
mod tests;
