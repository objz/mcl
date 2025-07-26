use config::{Config as ConfigLoader, File};
use once_cell::sync::Lazy;
use ratatui::style::Color;
use serde::{Deserialize, Deserializer};

use crate::config::get_config_path;

#[derive(Debug, Deserialize)]
pub struct ThemeConfig {
    pub colors: Colors,
}

#[derive(Debug, Deserialize)]
pub struct Colors {
    #[serde(deserialize_with = "deserialize_color")]
    pub background: Color,

    #[serde(deserialize_with = "deserialize_color")]
    pub foreground: Color,

    #[serde(deserialize_with = "deserialize_color")]
    pub border_focused: Color,

    #[serde(deserialize_with = "deserialize_color")]
    pub border_unfocused: Color,

    #[serde(deserialize_with = "deserialize_color")]
    pub row_highlight: Color,

    #[serde(deserialize_with = "deserialize_color")]
    pub row_background: Color,

    #[serde(deserialize_with = "deserialize_color")]
    pub row_alternate_bg: Color,
}

pub fn parse_color(color: &str) -> Color {
    match color.to_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "white" => Color::White,
        "gray" | "grey" => Color::Gray,
        "darkgray" | "darkgrey" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        "reset" => Color::Reset,
        hex if hex.starts_with('#') && hex.len() == 7 => {
            let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or_else(|_| {
                tracing::warn!("Invalid hex color format: {}", color);
                255
            });
            let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or_else(|_| {
                tracing::warn!("Invalid hex color format: {}", color);
                255
            });
            let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or_else(|_| {
                tracing::warn!("Invalid hex color format: {}", color);
                255
            });
            Color::Rgb(r, g, b)
        }
        _ => {
            tracing::warn!("Unknown color '{}', using White as fallback", color);
            Color::White
        }
    }
}

fn deserialize_color<'de, D>(deserializer: D) -> Result<Color, D::Error>
where
    D: Deserializer<'de>,
{
    let color_str: String = match Deserialize::deserialize(deserializer) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to deserialize color field: {}", e);
            return Ok(Color::White);
        }
    };
    Ok(parse_color(&color_str))
}

impl Default for Colors {
    fn default() -> Self {
        Colors {
            background: Color::Reset,
            foreground: Color::White,
            border_focused: Color::White,
            border_unfocused: Color::DarkGray,
            row_highlight: Color::White,
            row_background: Color::Reset,
            row_alternate_bg: Color::Reset,
        }
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        ThemeConfig {
            colors: Colors::default(),
        }
    }
}

pub static THEME: Lazy<ThemeConfig> = Lazy::new(|| {
    let config_path = get_config_path().join("config.toml");

    let built = match ConfigLoader::builder()
        .add_source(File::from(config_path.clone()).required(false))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                "Failed to load theme config from '{}': {}. Using default colors.",
                config_path.display(),
                e
            );
            return ThemeConfig::default();
        }
    };

    match built.try_deserialize::<ThemeConfig>() {
        Ok(theme) => theme,
        Err(e) => {
            tracing::warn!("Failed to parse theme config: {}. Using default colors.", e);
            ThemeConfig::default()
        }
    }
});
