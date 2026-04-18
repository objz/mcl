use std::path::Path;
use std::sync::LazyLock;

use ratatui::style::Color;
use ratatui::widgets::BorderType;
use ratatui_themekit::{CustomTheme, Theme, resolve_theme};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum BorderStyle {
    #[default]
    Rounded,
    Plain,
    Double,
    Thick,
}

impl BorderStyle {
    pub fn to_border_type(&self) -> BorderType {
        match self {
            Self::Rounded => BorderType::Rounded,
            Self::Plain => BorderType::Plain,
            Self::Double => BorderType::Double,
            Self::Thick => BorderType::Thick,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub struct ThemeOverrides {
    pub accent: Option<Color>,
    pub accent_dim: Option<Color>,
    pub text: Option<Color>,
    pub text_dim: Option<Color>,
    pub text_bright: Option<Color>,
    pub success: Option<Color>,
    pub error: Option<Color>,
    pub warning: Option<Color>,
    pub info: Option<Color>,
    pub diff_added: Option<Color>,
    pub diff_removed: Option<Color>,
    pub diff_context: Option<Color>,
    pub border: Option<Color>,
    pub surface: Option<Color>,
    pub background: Option<Color>,
}

#[derive(Debug, Deserialize)]
pub struct ThemeConfig {
    #[serde(default)]
    pub border_style: BorderStyle,
    #[serde(default = "default_theme_name")]
    pub theme: String,
    #[serde(default)]
    pub custom: Option<ThemeOverrides>,
}

fn default_theme_name() -> String {
    "catppuccin".to_owned()
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            border_style: BorderStyle::default(),
            theme: default_theme_name(),
            custom: None,
        }
    }
}

fn load_theme_config() -> ThemeConfig {
    let path = super::get_config_path().join("theme.toml");
    ensure_theme_exists(&path);
    match std::fs::read_to_string(&path) {
        Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
            tracing::warn!("Failed to parse theme.toml: {}. Using defaults.", e);
            ThemeConfig::default()
        }),
        Err(_) => ThemeConfig::default(),
    }
}

fn ensure_theme_exists(path: &Path) {
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, include_str!("../../assets/theme.toml"));
}

fn resolve_app_theme(config: &ThemeConfig) -> Box<dyn Theme> {
    let base = load_base_theme(&config.theme);

    let Some(overrides) = &config.custom else {
        return base;
    };

    Box::new(CustomTheme {
        name: format!("{} (customized)", base.name()),
        id: base.id().to_owned(),
        accent: overrides.accent.unwrap_or_else(|| base.accent()),
        accent_dim: overrides.accent_dim.unwrap_or_else(|| base.accent_dim()),
        text: overrides.text.unwrap_or_else(|| base.text()),
        text_dim: overrides.text_dim.unwrap_or_else(|| base.text_dim()),
        text_bright: overrides.text_bright.unwrap_or_else(|| base.text_bright()),
        success: overrides.success.unwrap_or_else(|| base.success()),
        error: overrides.error.unwrap_or_else(|| base.error()),
        warning: overrides.warning.unwrap_or_else(|| base.warning()),
        info: overrides.info.unwrap_or_else(|| base.info()),
        diff_added: overrides.diff_added.unwrap_or_else(|| base.diff_added()),
        diff_removed: overrides.diff_removed.unwrap_or_else(|| base.diff_removed()),
        diff_context: overrides.diff_context.unwrap_or_else(|| base.diff_context()),
        border: overrides.border.unwrap_or_else(|| base.border()),
        surface: overrides.surface.unwrap_or_else(|| base.surface()),
        background: overrides.background.unwrap_or_else(|| base.background()),
    })
}

/// Load a base theme by built-in name or file path.
fn load_base_theme(name: &str) -> Box<dyn Theme> {
    // Try as a file path first (absolute or relative to config dir)
    let path = if Path::new(name).is_absolute() {
        Some(std::path::PathBuf::from(name))
    } else {
        // Check in the mcl config theme directory
        let theme_dir = super::get_config_path().join("theme");
        let candidate = theme_dir.join(name);
        if candidate.exists() {
            Some(candidate)
        } else {
            let with_ext = theme_dir.join(format!("{name}.toml"));
            if with_ext.exists() {
                Some(with_ext)
            } else {
                None
            }
        }
    };

    if let Some(path) = path {
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(custom) = toml::from_str::<CustomTheme>(&content) {
                return Box::new(custom);
            } else {
                tracing::warn!("Failed to parse theme file: {}", path.display());
            }
        }
    }

    // Fall back to built-in theme by name
    resolve_theme(name)
}

static THEME_CONFIG: LazyLock<ThemeConfig> = LazyLock::new(load_theme_config);

pub static THEME: LazyLock<Box<dyn Theme>> = LazyLock::new(|| resolve_app_theme(&THEME_CONFIG));

pub static BORDER_STYLE: LazyLock<BorderStyle> =
    LazyLock::new(|| THEME_CONFIG.border_style.clone());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_config() {
        let config = ThemeConfig::default();
        assert_eq!(config.theme, "catppuccin");
        assert_eq!(config.border_style, BorderStyle::Rounded);
        assert!(config.custom.is_none());
    }

    #[test]
    fn border_style_roundtrip() {
        let style = BorderStyle::Double;
        assert_eq!(style.to_border_type(), BorderType::Double);
    }

    #[test]
    fn theme_config_deserialize_builtin() {
        let toml_str = r#"
theme = "dracula"
border_style = "plain"
"#;
        let config: ThemeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.theme, "dracula");
        assert_eq!(config.border_style, BorderStyle::Plain);
        assert!(config.custom.is_none());
    }

    #[test]
    fn theme_config_with_partial_overrides() {
        let toml_str = r#"
theme = "dracula"

[custom]
accent = "Red"
"#;
        let config: ThemeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.theme, "dracula");
        let overrides = config.custom.unwrap();
        assert_eq!(overrides.accent, Some(Color::Red));
        assert!(overrides.text.is_none());
    }

    #[test]
    fn resolve_with_overrides_keeps_base() {
        let config = ThemeConfig {
            theme: "dracula".to_owned(),
            custom: Some(ThemeOverrides {
                accent: Some(Color::Red),
                ..ThemeOverrides::default()
            }),
            ..ThemeConfig::default()
        };
        let theme = resolve_app_theme(&config);
        assert_eq!(theme.accent(), Color::Red);
        // Other colors should come from dracula base
        let base = resolve_theme("dracula");
        assert_eq!(theme.text(), base.text());
        assert_eq!(theme.error(), base.error());
    }

    #[test]
    fn resolve_builtin_theme() {
        let config = ThemeConfig {
            theme: "dracula".to_owned(),
            ..ThemeConfig::default()
        };
        let theme = resolve_app_theme(&config);
        assert_eq!(theme.id(), "dracula");
    }

    #[test]
    fn theme_config_empty_toml_uses_defaults() {
        let config: ThemeConfig = toml::from_str("").unwrap();
        assert_eq!(config.theme, "catppuccin");
        assert_eq!(config.border_style, BorderStyle::Rounded);
    }
}
