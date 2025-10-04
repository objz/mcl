use dirs_next;
use ratatui::style::Color;
use serde::{Deserialize, Deserializer};

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
    #[serde(default = "default_meta_dir")]
    pub meta_dir: String,
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

#[derive(Debug, Deserialize, Clone)]
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
    #[serde(deserialize_with = "deserialize_color", default = "default_popup_bg")]
    pub popup_bg: Color,
    #[serde(deserialize_with = "deserialize_color", default = "default_error")]
    pub error: Color,
    #[serde(deserialize_with = "deserialize_color", default = "default_warn")]
    pub warn: Color,
    #[serde(deserialize_with = "deserialize_color", default = "default_success")]
    pub success: Color,
    #[serde(deserialize_with = "deserialize_color", default = "default_accent")]
    pub accent: Color,
    #[serde(deserialize_with = "deserialize_color", default = "default_text_idle")]
    pub text_idle: Color,
    #[serde(
        deserialize_with = "deserialize_color",
        default = "default_progress_fill"
    )]
    pub progress_fill: Color,
    #[serde(
        deserialize_with = "deserialize_color",
        default = "default_progress_track"
    )]
    pub progress_track: Color,
    #[serde(deserialize_with = "deserialize_color", default = "default_badge_text")]
    pub badge_text: Color,
    #[serde(deserialize_with = "deserialize_color", default = "default_fade_to")]
    pub fade_to: Color,
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
            popup_bg: default_popup_bg(),
            error: default_error(),
            warn: default_warn(),
            success: default_success(),
            accent: default_accent(),
            text_idle: default_text_idle(),
            progress_fill: default_progress_fill(),
            progress_track: default_progress_track(),
            badge_text: default_badge_text(),
            fade_to: default_fade_to(),
        }
    }
}

fn default_popup_bg() -> Color {
    Color::Rgb(0x1e, 0x1e, 0x1e)
}

fn default_error() -> Color {
    Color::Red
}

fn default_warn() -> Color {
    Color::Yellow
}

fn default_success() -> Color {
    Color::Green
}

fn default_accent() -> Color {
    Color::Yellow
}

fn default_text_idle() -> Color {
    Color::DarkGray
}

fn default_progress_fill() -> Color {
    Color::Green
}

fn default_progress_track() -> Color {
    Color::DarkGray
}

fn default_badge_text() -> Color {
    Color::Black
}

fn default_fade_to() -> Color {
    Color::DarkGray
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
                tracing::warn!("Invalid hex color: {}", color);
                255
            });
            let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or_else(|_| {
                tracing::warn!("Invalid hex color: {}", color);
                255
            });
            let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or_else(|_| {
                tracing::warn!("Invalid hex color: {}", color);
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

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub paths: Paths,
    #[serde(default)]
    pub defaults: Defaults,
    #[serde(default)]
    pub colors: Colors,
}
