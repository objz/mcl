use serde::{Deserialize, Deserializer};
use ratatui::style::Color;
use crate::error;

#[derive(Debug, Deserialize)]
pub struct General {
    pub debug: bool,
}

#[derive(Debug, Deserialize)]
pub struct Colors {
    #[serde(deserialize_with = "deserialize_color")]
    pub background: Color,        // General background color

    #[serde(deserialize_with = "deserialize_color")]
    pub foreground: Color,        // General text color

    #[serde(deserialize_with = "deserialize_color")]
    pub border_focused: Color,    // Border color for focused areas

    #[serde(deserialize_with = "deserialize_color")]
    pub border_unfocused: Color,  // Border color for unfocused areas

    #[serde(deserialize_with = "deserialize_color")]
    pub row_highlight: Color,     // Highlighted row color

    #[serde(deserialize_with = "deserialize_color")]
    pub row_alternate_bg: Color,  // Alternate row background color

    #[serde(deserialize_with = "deserialize_color")]
    pub scrollbar: Color,         // Scrollbar color
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub general: General,
    pub colors: Colors,
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
                error!("Invalid hex color format: {}", color);
                255
            });
            let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or_else(|_| {
                error!("Invalid hex color format: {}", color);
                255
            });
            let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or_else(|_| {
                error!("Invalid hex color format: {}", color);
                255
            });
            Color::Rgb(r, g, b)
        }
        _ => {
            error!("Invalid color name or hex value: {}", color);
            panic!("Invalid config");
        }
    }
}



fn deserialize_color<'de, D>(deserializer: D) -> Result<Color, D::Error>
where
    D: Deserializer<'de>,
{
    let color_str: String = Deserialize::deserialize(deserializer)?;
    Ok(parse_color(&color_str))
}
