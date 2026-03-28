use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use once_cell::sync::Lazy;
use ratatui::{style::Color, widgets::BorderType};
use serde::{Deserialize, Serialize};

pub fn get_theme_path() -> PathBuf {
    dirs_next::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mcl")
        .join("theme.toml")
}

pub fn ensure_theme_exists(path: &Path) {
    if path.exists() {
        return;
    }
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!("Failed to create theme config directory: {}", e);
            return;
        }
    }
    if let Err(e) = std::fs::write(path, include_str!("../../assets/theme.toml")) {
        tracing::warn!("Failed to write default theme.toml: {}", e);
    }
}

fn resolve_color(s: &str, palette: &HashMap<String, Color>) -> Color {
    palette
        .get(s)
        .copied()
        .unwrap_or_else(|| s.parse::<Color>().unwrap_or(Color::White))
}

pub fn load_theme_from_path(path: &Path) -> Theme {
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return Theme::default(),
    };

    let raw: RawTheme = match toml::from_str(&content) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Failed to parse theme.toml: {}. Using defaults.", e);
            return Theme::default();
        }
    };

    resolve_theme(raw)
}

pub fn load_theme() -> Theme {
    let path = get_theme_path();
    ensure_theme_exists(&path);
    load_theme_from_path(&path)
}

pub static THEME: Lazy<Theme> = Lazy::new(load_theme);

macro_rules! color_default {
    ($name:ident, $value:expr) => {
        fn $name() -> Color {
            $value
        }
    };
}

macro_rules! bool_default {
    ($name:ident, $value:expr) => {
        fn $name() -> bool {
            $value
        }
    };
}

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
            BorderStyle::Rounded => BorderType::Rounded,
            BorderStyle::Plain => BorderType::Plain,
            BorderStyle::Double => BorderType::Double,
            BorderStyle::Thick => BorderType::Thick,
        }
    }
}

impl std::str::FromStr for BorderStyle {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "rounded" => Ok(Self::Rounded),
            "plain" => Ok(Self::Plain),
            "double" => Ok(Self::Double),
            "thick" => Ok(Self::Thick),
            _ => Err(()),
        }
    }
}

color_default!(default_general_bg, Color::Rgb(0x1e, 0x1e, 0x1e));
color_default!(default_general_fg, Color::White);
color_default!(default_general_accent, Color::Yellow);
color_default!(default_general_text_secondary, Color::DarkGray);
color_default!(default_general_error, Color::Red);
color_default!(default_general_warn, Color::Yellow);
color_default!(default_general_success, Color::Green);

color_default!(default_profiles_border_focused_fg, Color::White);
color_default!(default_profiles_border_unfocused_fg, Color::DarkGray);
color_default!(default_profiles_selected_fg, Color::Yellow);
color_default!(default_profiles_selected_bg, Color::Rgb(0x1a, 0x1a, 0x1a));
bool_default!(default_profiles_selected_bold, true);
color_default!(default_profiles_row_alt_bg, Color::Rgb(0x24, 0x24, 0x24));
color_default!(default_profiles_text_fg, Color::White);
color_default!(default_profiles_running_fg, Color::Green);
color_default!(default_profiles_loader_vanilla, Color::Green);
color_default!(default_profiles_loader_fabric, Color::Rgb(0x71, 0xa5, 0xde));
color_default!(default_profiles_loader_forge, Color::Rgb(0xe0, 0x7b, 0x39));
color_default!(
    default_profiles_loader_neoforge,
    Color::Rgb(0xe0, 0x54, 0x1b)
);
color_default!(default_profiles_loader_quilt, Color::Rgb(0xa6, 0x5c, 0xcb));

color_default!(default_content_border_focused_fg, Color::White);
color_default!(default_content_border_unfocused_fg, Color::DarkGray);
color_default!(default_content_tab_active_fg, Color::Yellow);
bool_default!(default_content_tab_active_bold, true);
color_default!(default_content_tab_inactive_fg, Color::DarkGray);
color_default!(default_content_selected_bg, Color::Rgb(0x1a, 0x1a, 0x1a));
color_default!(default_content_text_fg, Color::White);

color_default!(default_content_list_border_focused_fg, Color::White);
color_default!(default_content_list_selected_fg, Color::Yellow);
color_default!(
    default_content_list_selected_bg,
    Color::Rgb(0x1a, 0x1a, 0x1a)
);
bool_default!(default_content_list_selected_bold, true);
color_default!(
    default_content_list_row_alt_bg,
    Color::Rgb(0x24, 0x24, 0x24)
);
color_default!(default_content_list_text_fg, Color::White);
color_default!(default_content_list_text_secondary_fg, Color::DarkGray);
bool_default!(default_content_list_disabled_crossed_out, true);

color_default!(default_details_border_focused_fg, Color::White);
color_default!(default_details_border_unfocused_fg, Color::DarkGray);
color_default!(default_details_label_fg, Color::DarkGray);
color_default!(default_details_value_fg, Color::White);

color_default!(default_logs_border_focused_fg, Color::White);
color_default!(default_logs_border_unfocused_fg, Color::DarkGray);
color_default!(default_logs_error_fg, Color::Red);
color_default!(default_logs_warn_fg, Color::Yellow);
color_default!(default_logs_info_fg, Color::White);
color_default!(default_logs_debug_fg, Color::DarkGray);
color_default!(default_logs_trace_fg, Color::DarkGray);
color_default!(default_logs_selected_fg, Color::Yellow);
color_default!(default_logs_selected_bg, Color::Rgb(0x1a, 0x1a, 0x1a));
bool_default!(default_logs_selected_bold, true);
color_default!(default_logs_row_alt_bg, Color::Rgb(0x24, 0x24, 0x24));
color_default!(default_logs_running_fg, Color::Green);
color_default!(default_logs_text_fg, Color::White);

color_default!(default_log_overlay_bg, Color::Rgb(0x1e, 0x1e, 0x1e));
color_default!(default_log_overlay_border_fg, Color::White);
color_default!(default_log_overlay_error_fg, Color::Red);
color_default!(default_log_overlay_warn_fg, Color::Yellow);
color_default!(default_log_overlay_info_fg, Color::White);
color_default!(default_log_overlay_debug_fg, Color::DarkGray);
color_default!(default_log_overlay_trace_fg, Color::DarkGray);
color_default!(default_log_overlay_text_fg, Color::White);

color_default!(default_status_border_focused_fg, Color::White);
color_default!(default_status_border_unfocused_fg, Color::DarkGray);
color_default!(default_status_label_fg, Color::DarkGray);
color_default!(default_status_text_fg, Color::White);
color_default!(default_status_progress_fill_fg, Color::Green);
color_default!(default_status_progress_track_fg, Color::DarkGray);

color_default!(default_search_border_focused_fg, Color::White);
color_default!(default_search_border_unfocused_fg, Color::DarkGray);
color_default!(default_search_highlight_fg, Color::Yellow);
bool_default!(default_search_highlight_bold, false);
bool_default!(default_search_highlight_underline, true);

color_default!(default_screenshots_border_focused_fg, Color::White);
color_default!(default_screenshots_label_fg, Color::DarkGray);
color_default!(default_screenshots_selected_border_fg, Color::Yellow);

color_default!(default_account_border_focused_fg, Color::White);
color_default!(default_account_border_unfocused_fg, Color::DarkGray);
color_default!(default_account_selected_fg, Color::Yellow);
color_default!(default_account_selected_bg, Color::Rgb(0x1a, 0x1a, 0x1a));
bool_default!(default_account_selected_bold, true);
color_default!(default_account_text_fg, Color::White);
color_default!(default_account_text_secondary_fg, Color::DarkGray);
color_default!(default_account_active_fg, Color::Green);
color_default!(default_account_popup_bg, Color::Rgb(0x1e, 0x1e, 0x1e));
color_default!(default_account_popup_border_fg, Color::White);

color_default!(default_popup_bg, Color::Rgb(0x1e, 0x1e, 0x1e));
color_default!(default_popup_border_fg, Color::White);
color_default!(default_popup_text_fg, Color::White);
color_default!(default_popup_keybind_active_fg, Color::White);
bool_default!(default_popup_keybind_active_bold, true);
color_default!(default_popup_keybind_inactive_fg, Color::DarkGray);

color_default!(default_popup_confirm_border_fg, Color::White);
color_default!(
    default_popup_confirm_highlight_bg,
    Color::Rgb(0x24, 0x24, 0x24)
);
color_default!(default_popup_confirm_text_fg, Color::White);

color_default!(default_popup_error_bg, Color::Rgb(0x1e, 0x1e, 0x1e));
color_default!(default_popup_error_border_fg, Color::White);
color_default!(default_popup_error_error_fg, Color::Red);
color_default!(default_popup_error_warn_fg, Color::Yellow);
color_default!(default_popup_error_text_fg, Color::White);
color_default!(default_popup_error_badge_bg, Color::Red);
color_default!(default_popup_error_badge_text_fg, Color::Black);

color_default!(default_popup_new_instance_bg, Color::Rgb(0x1e, 0x1e, 0x1e));
color_default!(default_popup_new_instance_border_fg, Color::White);
color_default!(
    default_popup_new_instance_field_active_border_fg,
    Color::White
);
color_default!(
    default_popup_new_instance_field_inactive_border_fg,
    Color::DarkGray
);
color_default!(default_popup_new_instance_text_fg, Color::White);
color_default!(default_popup_new_instance_error_fg, Color::Red);
color_default!(default_popup_new_instance_accent_fg, Color::Yellow);

color_default!(default_popup_import_bg, Color::Rgb(0x1e, 0x1e, 0x1e));
color_default!(default_popup_import_border_fg, Color::Rgb(0x71, 0xa5, 0xde));
color_default!(default_popup_import_text_fg, Color::White);
color_default!(default_popup_import_label_fg, Color::DarkGray);
color_default!(default_popup_import_accent_fg, Color::Rgb(0x71, 0xa5, 0xde));
color_default!(default_popup_import_cursor_fg, Color::Rgb(0x71, 0xa5, 0xde));
color_default!(default_popup_import_placeholder_fg, Color::DarkGray);

fn default_general_border_type() -> BorderStyle {
    BorderStyle::Rounded
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GeneralTheme {
    #[serde(default = "default_general_border_type")]
    pub border_type: BorderStyle,
    #[serde(default = "default_general_fg")]
    pub fg: Color,
    #[serde(default = "default_general_bg")]
    pub bg: Color,
    #[serde(default = "default_general_accent")]
    pub accent: Color,
    #[serde(default = "default_general_text_secondary")]
    pub text_secondary: Color,
    #[serde(default = "default_general_error")]
    pub error: Color,
    #[serde(default = "default_general_warn")]
    pub warn: Color,
    #[serde(default = "default_general_success")]
    pub success: Color,
}

impl Default for GeneralTheme {
    fn default() -> Self {
        Self {
            border_type: default_general_border_type(),
            fg: default_general_fg(),
            bg: default_general_bg(),
            accent: default_general_accent(),
            text_secondary: default_general_text_secondary(),
            error: default_general_error(),
            warn: default_general_warn(),
            success: default_general_success(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfilesTheme {
    #[serde(default = "default_profiles_border_focused_fg")]
    pub border_focused_fg: Color,
    #[serde(default = "default_profiles_border_unfocused_fg")]
    pub border_unfocused_fg: Color,
    #[serde(default = "default_profiles_selected_fg")]
    pub selected_fg: Color,
    #[serde(default = "default_profiles_selected_bg")]
    pub selected_bg: Color,
    #[serde(default = "default_profiles_selected_bold")]
    pub selected_bold: bool,
    #[serde(default = "default_profiles_row_alt_bg")]
    pub row_alt_bg: Color,
    #[serde(default = "default_profiles_text_fg")]
    pub text_fg: Color,
    #[serde(default = "default_profiles_running_fg")]
    pub running_fg: Color,
    #[serde(default = "default_profiles_loader_vanilla")]
    pub loader_vanilla: Color,
    #[serde(default = "default_profiles_loader_fabric")]
    pub loader_fabric: Color,
    #[serde(default = "default_profiles_loader_forge")]
    pub loader_forge: Color,
    #[serde(default = "default_profiles_loader_neoforge")]
    pub loader_neoforge: Color,
    #[serde(default = "default_profiles_loader_quilt")]
    pub loader_quilt: Color,
}

impl Default for ProfilesTheme {
    fn default() -> Self {
        Self {
            border_focused_fg: default_profiles_border_focused_fg(),
            border_unfocused_fg: default_profiles_border_unfocused_fg(),
            selected_fg: default_profiles_selected_fg(),
            selected_bg: default_profiles_selected_bg(),
            selected_bold: default_profiles_selected_bold(),
            row_alt_bg: default_profiles_row_alt_bg(),
            text_fg: default_profiles_text_fg(),
            running_fg: default_profiles_running_fg(),
            loader_vanilla: default_profiles_loader_vanilla(),
            loader_fabric: default_profiles_loader_fabric(),
            loader_forge: default_profiles_loader_forge(),
            loader_neoforge: default_profiles_loader_neoforge(),
            loader_quilt: default_profiles_loader_quilt(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContentTheme {
    #[serde(default = "default_content_border_focused_fg")]
    pub border_focused_fg: Color,
    #[serde(default = "default_content_border_unfocused_fg")]
    pub border_unfocused_fg: Color,
    #[serde(default = "default_content_tab_active_fg")]
    pub tab_active_fg: Color,
    #[serde(default = "default_content_tab_active_bold")]
    pub tab_active_bold: bool,
    #[serde(default = "default_content_tab_inactive_fg")]
    pub tab_inactive_fg: Color,
    #[serde(default = "default_content_selected_bg")]
    pub selected_bg: Color,
    #[serde(default = "default_content_text_fg")]
    pub text_fg: Color,
}

impl Default for ContentTheme {
    fn default() -> Self {
        Self {
            border_focused_fg: default_content_border_focused_fg(),
            border_unfocused_fg: default_content_border_unfocused_fg(),
            tab_active_fg: default_content_tab_active_fg(),
            tab_active_bold: default_content_tab_active_bold(),
            tab_inactive_fg: default_content_tab_inactive_fg(),
            selected_bg: default_content_selected_bg(),
            text_fg: default_content_text_fg(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContentListTheme {
    #[serde(default = "default_content_list_border_focused_fg")]
    pub border_focused_fg: Color,
    #[serde(default = "default_content_list_selected_fg")]
    pub selected_fg: Color,
    #[serde(default = "default_content_list_selected_bg")]
    pub selected_bg: Color,
    #[serde(default = "default_content_list_selected_bold")]
    pub selected_bold: bool,
    #[serde(default = "default_content_list_row_alt_bg")]
    pub row_alt_bg: Color,
    #[serde(default = "default_content_list_text_fg")]
    pub text_fg: Color,
    #[serde(default = "default_content_list_text_secondary_fg")]
    pub text_secondary_fg: Color,
    #[serde(default = "default_content_list_disabled_crossed_out")]
    pub disabled_crossed_out: bool,
}

impl Default for ContentListTheme {
    fn default() -> Self {
        Self {
            border_focused_fg: default_content_list_border_focused_fg(),
            selected_fg: default_content_list_selected_fg(),
            selected_bg: default_content_list_selected_bg(),
            selected_bold: default_content_list_selected_bold(),
            row_alt_bg: default_content_list_row_alt_bg(),
            text_fg: default_content_list_text_fg(),
            text_secondary_fg: default_content_list_text_secondary_fg(),
            disabled_crossed_out: default_content_list_disabled_crossed_out(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DetailsTheme {
    #[serde(default = "default_details_border_focused_fg")]
    pub border_focused_fg: Color,
    #[serde(default = "default_details_border_unfocused_fg")]
    pub border_unfocused_fg: Color,
    #[serde(default = "default_details_label_fg")]
    pub label_fg: Color,
    #[serde(default = "default_details_value_fg")]
    pub value_fg: Color,
}

impl Default for DetailsTheme {
    fn default() -> Self {
        Self {
            border_focused_fg: default_details_border_focused_fg(),
            border_unfocused_fg: default_details_border_unfocused_fg(),
            label_fg: default_details_label_fg(),
            value_fg: default_details_value_fg(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogsTheme {
    #[serde(default = "default_logs_border_focused_fg")]
    pub border_focused_fg: Color,
    #[serde(default = "default_logs_border_unfocused_fg")]
    pub border_unfocused_fg: Color,
    #[serde(default = "default_logs_error_fg")]
    pub error_fg: Color,
    #[serde(default = "default_logs_warn_fg")]
    pub warn_fg: Color,
    #[serde(default = "default_logs_info_fg")]
    pub info_fg: Color,
    #[serde(default = "default_logs_debug_fg")]
    pub debug_fg: Color,
    #[serde(default = "default_logs_trace_fg")]
    pub trace_fg: Color,
    #[serde(default = "default_logs_selected_fg")]
    pub selected_fg: Color,
    #[serde(default = "default_logs_selected_bg")]
    pub selected_bg: Color,
    #[serde(default = "default_logs_selected_bold")]
    pub selected_bold: bool,
    #[serde(default = "default_logs_row_alt_bg")]
    pub row_alt_bg: Color,
    #[serde(default = "default_logs_running_fg")]
    pub running_fg: Color,
    #[serde(default = "default_logs_text_fg")]
    pub text_fg: Color,
}

impl Default for LogsTheme {
    fn default() -> Self {
        Self {
            border_focused_fg: default_logs_border_focused_fg(),
            border_unfocused_fg: default_logs_border_unfocused_fg(),
            error_fg: default_logs_error_fg(),
            warn_fg: default_logs_warn_fg(),
            info_fg: default_logs_info_fg(),
            debug_fg: default_logs_debug_fg(),
            trace_fg: default_logs_trace_fg(),
            selected_fg: default_logs_selected_fg(),
            selected_bg: default_logs_selected_bg(),
            selected_bold: default_logs_selected_bold(),
            row_alt_bg: default_logs_row_alt_bg(),
            running_fg: default_logs_running_fg(),
            text_fg: default_logs_text_fg(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LogOverlayTheme {
    #[serde(default = "default_log_overlay_bg")]
    pub bg: Color,
    #[serde(default = "default_log_overlay_border_fg")]
    pub border_fg: Color,
    #[serde(default = "default_log_overlay_error_fg")]
    pub error_fg: Color,
    #[serde(default = "default_log_overlay_warn_fg")]
    pub warn_fg: Color,
    #[serde(default = "default_log_overlay_info_fg")]
    pub info_fg: Color,
    #[serde(default = "default_log_overlay_debug_fg")]
    pub debug_fg: Color,
    #[serde(default = "default_log_overlay_trace_fg")]
    pub trace_fg: Color,
    #[serde(default = "default_log_overlay_text_fg")]
    pub text_fg: Color,
}

impl Default for LogOverlayTheme {
    fn default() -> Self {
        Self {
            bg: default_log_overlay_bg(),
            border_fg: default_log_overlay_border_fg(),
            error_fg: default_log_overlay_error_fg(),
            warn_fg: default_log_overlay_warn_fg(),
            info_fg: default_log_overlay_info_fg(),
            debug_fg: default_log_overlay_debug_fg(),
            trace_fg: default_log_overlay_trace_fg(),
            text_fg: default_log_overlay_text_fg(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StatusTheme {
    #[serde(default = "default_status_border_focused_fg")]
    pub border_focused_fg: Color,
    #[serde(default = "default_status_border_unfocused_fg")]
    pub border_unfocused_fg: Color,
    #[serde(default = "default_status_label_fg")]
    pub label_fg: Color,
    #[serde(default = "default_status_text_fg")]
    pub text_fg: Color,
    #[serde(default = "default_status_progress_fill_fg")]
    pub progress_fill_fg: Color,
    #[serde(default = "default_status_progress_track_fg")]
    pub progress_track_fg: Color,
}

impl Default for StatusTheme {
    fn default() -> Self {
        Self {
            border_focused_fg: default_status_border_focused_fg(),
            border_unfocused_fg: default_status_border_unfocused_fg(),
            label_fg: default_status_label_fg(),
            text_fg: default_status_text_fg(),
            progress_fill_fg: default_status_progress_fill_fg(),
            progress_track_fg: default_status_progress_track_fg(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchTheme {
    #[serde(default = "default_search_border_focused_fg")]
    pub border_focused_fg: Color,
    #[serde(default = "default_search_border_unfocused_fg")]
    pub border_unfocused_fg: Color,
    #[serde(default = "default_search_highlight_fg")]
    pub highlight_fg: Color,
    #[serde(default = "default_search_highlight_bold")]
    pub highlight_bold: bool,
    #[serde(default = "default_search_highlight_underline")]
    pub highlight_underline: bool,
}

impl Default for SearchTheme {
    fn default() -> Self {
        Self {
            border_focused_fg: default_search_border_focused_fg(),
            border_unfocused_fg: default_search_border_unfocused_fg(),
            highlight_fg: default_search_highlight_fg(),
            highlight_bold: default_search_highlight_bold(),
            highlight_underline: default_search_highlight_underline(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScreenshotsTheme {
    #[serde(default = "default_screenshots_border_focused_fg")]
    pub border_focused_fg: Color,
    #[serde(default = "default_screenshots_label_fg")]
    pub label_fg: Color,
    #[serde(default = "default_screenshots_selected_border_fg")]
    pub selected_border_fg: Color,
}

impl Default for ScreenshotsTheme {
    fn default() -> Self {
        Self {
            border_focused_fg: default_screenshots_border_focused_fg(),
            label_fg: default_screenshots_label_fg(),
            selected_border_fg: default_screenshots_selected_border_fg(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountTheme {
    #[serde(default = "default_account_border_focused_fg")]
    pub border_focused_fg: Color,
    #[serde(default = "default_account_border_unfocused_fg")]
    pub border_unfocused_fg: Color,
    #[serde(default = "default_account_selected_fg")]
    pub selected_fg: Color,
    #[serde(default = "default_account_selected_bg")]
    pub selected_bg: Color,
    #[serde(default = "default_account_selected_bold")]
    pub selected_bold: bool,
    #[serde(default = "default_account_text_fg")]
    pub text_fg: Color,
    #[serde(default = "default_account_text_secondary_fg")]
    pub text_secondary_fg: Color,
    #[serde(default = "default_account_active_fg")]
    pub active_fg: Color,
    #[serde(default = "default_account_popup_bg")]
    pub popup_bg: Color,
    #[serde(default = "default_account_popup_border_fg")]
    pub popup_border_fg: Color,
}

impl Default for AccountTheme {
    fn default() -> Self {
        Self {
            border_focused_fg: default_account_border_focused_fg(),
            border_unfocused_fg: default_account_border_unfocused_fg(),
            selected_fg: default_account_selected_fg(),
            selected_bg: default_account_selected_bg(),
            selected_bold: default_account_selected_bold(),
            text_fg: default_account_text_fg(),
            text_secondary_fg: default_account_text_secondary_fg(),
            active_fg: default_account_active_fg(),
            popup_bg: default_account_popup_bg(),
            popup_border_fg: default_account_popup_border_fg(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PopupTheme {
    #[serde(default = "default_popup_bg")]
    pub bg: Color,
    #[serde(default = "default_popup_border_fg")]
    pub border_fg: Color,
    #[serde(default = "default_popup_text_fg")]
    pub text_fg: Color,
    #[serde(default = "default_popup_keybind_active_fg")]
    pub keybind_active_fg: Color,
    #[serde(default = "default_popup_keybind_active_bold")]
    pub keybind_active_bold: bool,
    #[serde(default = "default_popup_keybind_inactive_fg")]
    pub keybind_inactive_fg: Color,
}

impl Default for PopupTheme {
    fn default() -> Self {
        Self {
            bg: default_popup_bg(),
            border_fg: default_popup_border_fg(),
            text_fg: default_popup_text_fg(),
            keybind_active_fg: default_popup_keybind_active_fg(),
            keybind_active_bold: default_popup_keybind_active_bold(),
            keybind_inactive_fg: default_popup_keybind_inactive_fg(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PopupConfirmTheme {
    #[serde(default = "default_popup_confirm_border_fg")]
    pub border_fg: Color,
    #[serde(default = "default_popup_confirm_highlight_bg")]
    pub highlight_bg: Color,
    #[serde(default = "default_popup_confirm_text_fg")]
    pub text_fg: Color,
}

impl Default for PopupConfirmTheme {
    fn default() -> Self {
        Self {
            border_fg: default_popup_confirm_border_fg(),
            highlight_bg: default_popup_confirm_highlight_bg(),
            text_fg: default_popup_confirm_text_fg(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PopupErrorTheme {
    #[serde(default = "default_popup_error_bg")]
    pub bg: Color,
    #[serde(default = "default_popup_error_border_fg")]
    pub border_fg: Color,
    #[serde(default = "default_popup_error_error_fg")]
    pub error_fg: Color,
    #[serde(default = "default_popup_error_warn_fg")]
    pub warn_fg: Color,
    #[serde(default = "default_popup_error_text_fg")]
    pub text_fg: Color,
    #[serde(default = "default_popup_error_badge_bg")]
    pub badge_bg: Color,
    #[serde(default = "default_popup_error_badge_text_fg")]
    pub badge_text_fg: Color,
}

impl Default for PopupErrorTheme {
    fn default() -> Self {
        Self {
            bg: default_popup_error_bg(),
            border_fg: default_popup_error_border_fg(),
            error_fg: default_popup_error_error_fg(),
            warn_fg: default_popup_error_warn_fg(),
            text_fg: default_popup_error_text_fg(),
            badge_bg: default_popup_error_badge_bg(),
            badge_text_fg: default_popup_error_badge_text_fg(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PopupNewInstanceTheme {
    #[serde(default = "default_popup_new_instance_bg")]
    pub bg: Color,
    #[serde(default = "default_popup_new_instance_border_fg")]
    pub border_fg: Color,
    #[serde(default = "default_popup_new_instance_field_active_border_fg")]
    pub field_active_border_fg: Color,
    #[serde(default = "default_popup_new_instance_field_inactive_border_fg")]
    pub field_inactive_border_fg: Color,
    #[serde(default = "default_popup_new_instance_text_fg")]
    pub text_fg: Color,
    #[serde(default = "default_popup_new_instance_error_fg")]
    pub error_fg: Color,
    #[serde(default = "default_popup_new_instance_accent_fg")]
    pub accent_fg: Color,
}

impl Default for PopupNewInstanceTheme {
    fn default() -> Self {
        Self {
            bg: default_popup_new_instance_bg(),
            border_fg: default_popup_new_instance_border_fg(),
            field_active_border_fg: default_popup_new_instance_field_active_border_fg(),
            field_inactive_border_fg: default_popup_new_instance_field_inactive_border_fg(),
            text_fg: default_popup_new_instance_text_fg(),
            error_fg: default_popup_new_instance_error_fg(),
            accent_fg: default_popup_new_instance_accent_fg(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PopupImportTheme {
    #[serde(default = "default_popup_import_bg")]
    pub bg: Color,
    #[serde(default = "default_popup_import_border_fg")]
    pub border_fg: Color,
    #[serde(default = "default_popup_import_text_fg")]
    pub text_fg: Color,
    #[serde(default = "default_popup_import_label_fg")]
    pub label_fg: Color,
    #[serde(default = "default_popup_import_accent_fg")]
    pub accent_fg: Color,
    #[serde(default = "default_popup_import_cursor_fg")]
    pub cursor_fg: Color,
    #[serde(default = "default_popup_import_placeholder_fg")]
    pub placeholder_fg: Color,
}

impl Default for PopupImportTheme {
    fn default() -> Self {
        Self {
            bg: default_popup_import_bg(),
            border_fg: default_popup_import_border_fg(),
            text_fg: default_popup_import_text_fg(),
            label_fg: default_popup_import_label_fg(),
            accent_fg: default_popup_import_accent_fg(),
            cursor_fg: default_popup_import_cursor_fg(),
            placeholder_fg: default_popup_import_placeholder_fg(),
        }
    }
}

#[derive(Debug, Deserialize, Default)]
struct RawGeneralTheme {
    border_type: Option<String>,
    fg: Option<String>,
    bg: Option<String>,
    accent: Option<String>,
    text_secondary: Option<String>,
    error: Option<String>,
    warn: Option<String>,
    success: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawProfilesTheme {
    border_focused_fg: Option<String>,
    border_unfocused_fg: Option<String>,
    selected_fg: Option<String>,
    selected_bg: Option<String>,
    selected_bold: Option<bool>,
    row_alt_bg: Option<String>,
    text_fg: Option<String>,
    running_fg: Option<String>,
    loader_vanilla: Option<String>,
    loader_fabric: Option<String>,
    loader_forge: Option<String>,
    loader_neoforge: Option<String>,
    loader_quilt: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawContentTheme {
    border_focused_fg: Option<String>,
    border_unfocused_fg: Option<String>,
    tab_active_fg: Option<String>,
    tab_active_bold: Option<bool>,
    tab_inactive_fg: Option<String>,
    selected_bg: Option<String>,
    text_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawContentListTheme {
    border_focused_fg: Option<String>,
    selected_fg: Option<String>,
    selected_bg: Option<String>,
    selected_bold: Option<bool>,
    row_alt_bg: Option<String>,
    text_fg: Option<String>,
    text_secondary_fg: Option<String>,
    disabled_crossed_out: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct RawDetailsTheme {
    border_focused_fg: Option<String>,
    border_unfocused_fg: Option<String>,
    label_fg: Option<String>,
    value_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawLogsTheme {
    border_focused_fg: Option<String>,
    border_unfocused_fg: Option<String>,
    error_fg: Option<String>,
    warn_fg: Option<String>,
    info_fg: Option<String>,
    debug_fg: Option<String>,
    trace_fg: Option<String>,
    selected_fg: Option<String>,
    selected_bg: Option<String>,
    selected_bold: Option<bool>,
    row_alt_bg: Option<String>,
    running_fg: Option<String>,
    text_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawLogOverlayTheme {
    bg: Option<String>,
    border_fg: Option<String>,
    error_fg: Option<String>,
    warn_fg: Option<String>,
    info_fg: Option<String>,
    debug_fg: Option<String>,
    trace_fg: Option<String>,
    text_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawStatusTheme {
    border_focused_fg: Option<String>,
    border_unfocused_fg: Option<String>,
    label_fg: Option<String>,
    text_fg: Option<String>,
    progress_fill_fg: Option<String>,
    progress_track_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawSearchTheme {
    border_focused_fg: Option<String>,
    border_unfocused_fg: Option<String>,
    highlight_fg: Option<String>,
    highlight_bold: Option<bool>,
    highlight_underline: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct RawScreenshotsTheme {
    border_focused_fg: Option<String>,
    label_fg: Option<String>,
    selected_border_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawAccountTheme {
    border_focused_fg: Option<String>,
    border_unfocused_fg: Option<String>,
    selected_fg: Option<String>,
    selected_bg: Option<String>,
    selected_bold: Option<bool>,
    text_fg: Option<String>,
    text_secondary_fg: Option<String>,
    active_fg: Option<String>,
    popup_bg: Option<String>,
    popup_border_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawPopupTheme {
    bg: Option<String>,
    border_fg: Option<String>,
    text_fg: Option<String>,
    keybind_active_fg: Option<String>,
    keybind_active_bold: Option<bool>,
    keybind_inactive_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawPopupConfirmTheme {
    border_fg: Option<String>,
    highlight_bg: Option<String>,
    text_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawPopupErrorTheme {
    bg: Option<String>,
    border_fg: Option<String>,
    error_fg: Option<String>,
    warn_fg: Option<String>,
    text_fg: Option<String>,
    badge_bg: Option<String>,
    badge_text_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawPopupNewInstanceTheme {
    bg: Option<String>,
    border_fg: Option<String>,
    field_active_border_fg: Option<String>,
    field_inactive_border_fg: Option<String>,
    text_fg: Option<String>,
    error_fg: Option<String>,
    accent_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawPopupImportTheme {
    bg: Option<String>,
    border_fg: Option<String>,
    text_fg: Option<String>,
    label_fg: Option<String>,
    accent_fg: Option<String>,
    cursor_fg: Option<String>,
    placeholder_fg: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RawTheme {
    #[serde(default)]
    palette: HashMap<String, String>,
    #[serde(default)]
    general: RawGeneralTheme,
    #[serde(default)]
    profiles: RawProfilesTheme,
    #[serde(default)]
    content: RawContentTheme,
    #[serde(default)]
    content_list: RawContentListTheme,
    #[serde(default)]
    details: RawDetailsTheme,
    #[serde(default)]
    logs: RawLogsTheme,
    #[serde(default)]
    log_overlay: RawLogOverlayTheme,
    #[serde(default)]
    status: RawStatusTheme,
    #[serde(default)]
    search: RawSearchTheme,
    #[serde(default)]
    screenshots: RawScreenshotsTheme,
    #[serde(default)]
    account: RawAccountTheme,
    #[serde(default)]
    popup: RawPopupTheme,
    #[serde(default)]
    popup_confirm: RawPopupConfirmTheme,
    #[serde(default)]
    popup_error: RawPopupErrorTheme,
    #[serde(default)]
    popup_new_instance: RawPopupNewInstanceTheme,
    #[serde(default)]
    popup_import: RawPopupImportTheme,
}

fn resolve_theme(raw: RawTheme) -> Theme {
    let palette: HashMap<String, Color> = raw
        .palette
        .iter()
        .map(|(k, v)| (k.clone(), v.parse::<Color>().unwrap_or(Color::White)))
        .collect();

    let defaults = Theme::default();
    let rc = |opt: &Option<String>, default: Color| -> Color {
        opt.as_deref()
            .map(|s| resolve_color(s, &palette))
            .unwrap_or(default)
    };

    Theme {
        general: GeneralTheme {
            border_type: raw
                .general
                .border_type
                .as_deref()
                .and_then(|s| s.parse::<BorderStyle>().ok())
                .unwrap_or(defaults.general.border_type),
            fg: rc(&raw.general.fg, defaults.general.fg),
            bg: rc(&raw.general.bg, defaults.general.bg),
            accent: rc(&raw.general.accent, defaults.general.accent),
            text_secondary: rc(&raw.general.text_secondary, defaults.general.text_secondary),
            error: rc(&raw.general.error, defaults.general.error),
            warn: rc(&raw.general.warn, defaults.general.warn),
            success: rc(&raw.general.success, defaults.general.success),
        },
        profiles: ProfilesTheme {
            border_focused_fg: rc(
                &raw.profiles.border_focused_fg,
                defaults.profiles.border_focused_fg,
            ),
            border_unfocused_fg: rc(
                &raw.profiles.border_unfocused_fg,
                defaults.profiles.border_unfocused_fg,
            ),
            selected_fg: rc(&raw.profiles.selected_fg, defaults.profiles.selected_fg),
            selected_bg: rc(&raw.profiles.selected_bg, defaults.profiles.selected_bg),
            selected_bold: raw
                .profiles
                .selected_bold
                .unwrap_or(defaults.profiles.selected_bold),
            row_alt_bg: rc(&raw.profiles.row_alt_bg, defaults.profiles.row_alt_bg),
            text_fg: rc(&raw.profiles.text_fg, defaults.profiles.text_fg),
            running_fg: rc(&raw.profiles.running_fg, defaults.profiles.running_fg),
            loader_vanilla: rc(
                &raw.profiles.loader_vanilla,
                defaults.profiles.loader_vanilla,
            ),
            loader_fabric: rc(&raw.profiles.loader_fabric, defaults.profiles.loader_fabric),
            loader_forge: rc(&raw.profiles.loader_forge, defaults.profiles.loader_forge),
            loader_neoforge: rc(
                &raw.profiles.loader_neoforge,
                defaults.profiles.loader_neoforge,
            ),
            loader_quilt: rc(&raw.profiles.loader_quilt, defaults.profiles.loader_quilt),
        },
        content: ContentTheme {
            border_focused_fg: rc(
                &raw.content.border_focused_fg,
                defaults.content.border_focused_fg,
            ),
            border_unfocused_fg: rc(
                &raw.content.border_unfocused_fg,
                defaults.content.border_unfocused_fg,
            ),
            tab_active_fg: rc(&raw.content.tab_active_fg, defaults.content.tab_active_fg),
            tab_active_bold: raw
                .content
                .tab_active_bold
                .unwrap_or(defaults.content.tab_active_bold),
            tab_inactive_fg: rc(
                &raw.content.tab_inactive_fg,
                defaults.content.tab_inactive_fg,
            ),
            selected_bg: rc(&raw.content.selected_bg, defaults.content.selected_bg),
            text_fg: rc(&raw.content.text_fg, defaults.content.text_fg),
        },
        content_list: ContentListTheme {
            border_focused_fg: rc(
                &raw.content_list.border_focused_fg,
                defaults.content_list.border_focused_fg,
            ),
            selected_fg: rc(
                &raw.content_list.selected_fg,
                defaults.content_list.selected_fg,
            ),
            selected_bg: rc(
                &raw.content_list.selected_bg,
                defaults.content_list.selected_bg,
            ),
            selected_bold: raw
                .content_list
                .selected_bold
                .unwrap_or(defaults.content_list.selected_bold),
            row_alt_bg: rc(
                &raw.content_list.row_alt_bg,
                defaults.content_list.row_alt_bg,
            ),
            text_fg: rc(&raw.content_list.text_fg, defaults.content_list.text_fg),
            text_secondary_fg: rc(
                &raw.content_list.text_secondary_fg,
                defaults.content_list.text_secondary_fg,
            ),
            disabled_crossed_out: raw
                .content_list
                .disabled_crossed_out
                .unwrap_or(defaults.content_list.disabled_crossed_out),
        },
        details: DetailsTheme {
            border_focused_fg: rc(
                &raw.details.border_focused_fg,
                defaults.details.border_focused_fg,
            ),
            border_unfocused_fg: rc(
                &raw.details.border_unfocused_fg,
                defaults.details.border_unfocused_fg,
            ),
            label_fg: rc(&raw.details.label_fg, defaults.details.label_fg),
            value_fg: rc(&raw.details.value_fg, defaults.details.value_fg),
        },
        logs: LogsTheme {
            border_focused_fg: rc(&raw.logs.border_focused_fg, defaults.logs.border_focused_fg),
            border_unfocused_fg: rc(
                &raw.logs.border_unfocused_fg,
                defaults.logs.border_unfocused_fg,
            ),
            error_fg: rc(&raw.logs.error_fg, defaults.logs.error_fg),
            warn_fg: rc(&raw.logs.warn_fg, defaults.logs.warn_fg),
            info_fg: rc(&raw.logs.info_fg, defaults.logs.info_fg),
            debug_fg: rc(&raw.logs.debug_fg, defaults.logs.debug_fg),
            trace_fg: rc(&raw.logs.trace_fg, defaults.logs.trace_fg),
            selected_fg: rc(&raw.logs.selected_fg, defaults.logs.selected_fg),
            selected_bg: rc(&raw.logs.selected_bg, defaults.logs.selected_bg),
            selected_bold: raw
                .logs
                .selected_bold
                .unwrap_or(defaults.logs.selected_bold),
            row_alt_bg: rc(&raw.logs.row_alt_bg, defaults.logs.row_alt_bg),
            running_fg: rc(&raw.logs.running_fg, defaults.logs.running_fg),
            text_fg: rc(&raw.logs.text_fg, defaults.logs.text_fg),
        },
        log_overlay: LogOverlayTheme {
            bg: rc(&raw.log_overlay.bg, defaults.log_overlay.bg),
            border_fg: rc(&raw.log_overlay.border_fg, defaults.log_overlay.border_fg),
            error_fg: rc(&raw.log_overlay.error_fg, defaults.log_overlay.error_fg),
            warn_fg: rc(&raw.log_overlay.warn_fg, defaults.log_overlay.warn_fg),
            info_fg: rc(&raw.log_overlay.info_fg, defaults.log_overlay.info_fg),
            debug_fg: rc(&raw.log_overlay.debug_fg, defaults.log_overlay.debug_fg),
            trace_fg: rc(&raw.log_overlay.trace_fg, defaults.log_overlay.trace_fg),
            text_fg: rc(&raw.log_overlay.text_fg, defaults.log_overlay.text_fg),
        },
        status: StatusTheme {
            border_focused_fg: rc(
                &raw.status.border_focused_fg,
                defaults.status.border_focused_fg,
            ),
            border_unfocused_fg: rc(
                &raw.status.border_unfocused_fg,
                defaults.status.border_unfocused_fg,
            ),
            label_fg: rc(&raw.status.label_fg, defaults.status.label_fg),
            text_fg: rc(&raw.status.text_fg, defaults.status.text_fg),
            progress_fill_fg: rc(
                &raw.status.progress_fill_fg,
                defaults.status.progress_fill_fg,
            ),
            progress_track_fg: rc(
                &raw.status.progress_track_fg,
                defaults.status.progress_track_fg,
            ),
        },
        search: SearchTheme {
            border_focused_fg: rc(
                &raw.search.border_focused_fg,
                defaults.search.border_focused_fg,
            ),
            border_unfocused_fg: rc(
                &raw.search.border_unfocused_fg,
                defaults.search.border_unfocused_fg,
            ),
            highlight_fg: rc(&raw.search.highlight_fg, defaults.search.highlight_fg),
            highlight_bold: raw
                .search
                .highlight_bold
                .unwrap_or(defaults.search.highlight_bold),
            highlight_underline: raw
                .search
                .highlight_underline
                .unwrap_or(defaults.search.highlight_underline),
        },
        screenshots: ScreenshotsTheme {
            border_focused_fg: rc(
                &raw.screenshots.border_focused_fg,
                defaults.screenshots.border_focused_fg,
            ),
            label_fg: rc(&raw.screenshots.label_fg, defaults.screenshots.label_fg),
            selected_border_fg: rc(
                &raw.screenshots.selected_border_fg,
                defaults.screenshots.selected_border_fg,
            ),
        },
        account: AccountTheme {
            border_focused_fg: rc(
                &raw.account.border_focused_fg,
                defaults.account.border_focused_fg,
            ),
            border_unfocused_fg: rc(
                &raw.account.border_unfocused_fg,
                defaults.account.border_unfocused_fg,
            ),
            selected_fg: rc(&raw.account.selected_fg, defaults.account.selected_fg),
            selected_bg: rc(&raw.account.selected_bg, defaults.account.selected_bg),
            selected_bold: raw
                .account
                .selected_bold
                .unwrap_or(defaults.account.selected_bold),
            text_fg: rc(&raw.account.text_fg, defaults.account.text_fg),
            text_secondary_fg: rc(
                &raw.account.text_secondary_fg,
                defaults.account.text_secondary_fg,
            ),
            active_fg: rc(&raw.account.active_fg, defaults.account.active_fg),
            popup_bg: rc(&raw.account.popup_bg, defaults.account.popup_bg),
            popup_border_fg: rc(
                &raw.account.popup_border_fg,
                defaults.account.popup_border_fg,
            ),
        },
        popup: PopupTheme {
            bg: rc(&raw.popup.bg, defaults.popup.bg),
            border_fg: rc(&raw.popup.border_fg, defaults.popup.border_fg),
            text_fg: rc(&raw.popup.text_fg, defaults.popup.text_fg),
            keybind_active_fg: rc(
                &raw.popup.keybind_active_fg,
                defaults.popup.keybind_active_fg,
            ),
            keybind_active_bold: raw
                .popup
                .keybind_active_bold
                .unwrap_or(defaults.popup.keybind_active_bold),
            keybind_inactive_fg: rc(
                &raw.popup.keybind_inactive_fg,
                defaults.popup.keybind_inactive_fg,
            ),
        },
        popup_confirm: PopupConfirmTheme {
            border_fg: rc(
                &raw.popup_confirm.border_fg,
                defaults.popup_confirm.border_fg,
            ),
            highlight_bg: rc(
                &raw.popup_confirm.highlight_bg,
                defaults.popup_confirm.highlight_bg,
            ),
            text_fg: rc(&raw.popup_confirm.text_fg, defaults.popup_confirm.text_fg),
        },
        popup_error: PopupErrorTheme {
            bg: rc(&raw.popup_error.bg, defaults.popup_error.bg),
            border_fg: rc(&raw.popup_error.border_fg, defaults.popup_error.border_fg),
            error_fg: rc(&raw.popup_error.error_fg, defaults.popup_error.error_fg),
            warn_fg: rc(&raw.popup_error.warn_fg, defaults.popup_error.warn_fg),
            text_fg: rc(&raw.popup_error.text_fg, defaults.popup_error.text_fg),
            badge_bg: rc(&raw.popup_error.badge_bg, defaults.popup_error.badge_bg),
            badge_text_fg: rc(
                &raw.popup_error.badge_text_fg,
                defaults.popup_error.badge_text_fg,
            ),
        },
        popup_new_instance: PopupNewInstanceTheme {
            bg: rc(&raw.popup_new_instance.bg, defaults.popup_new_instance.bg),
            border_fg: rc(
                &raw.popup_new_instance.border_fg,
                defaults.popup_new_instance.border_fg,
            ),
            field_active_border_fg: rc(
                &raw.popup_new_instance.field_active_border_fg,
                defaults.popup_new_instance.field_active_border_fg,
            ),
            field_inactive_border_fg: rc(
                &raw.popup_new_instance.field_inactive_border_fg,
                defaults.popup_new_instance.field_inactive_border_fg,
            ),
            text_fg: rc(
                &raw.popup_new_instance.text_fg,
                defaults.popup_new_instance.text_fg,
            ),
            error_fg: rc(
                &raw.popup_new_instance.error_fg,
                defaults.popup_new_instance.error_fg,
            ),
            accent_fg: rc(
                &raw.popup_new_instance.accent_fg,
                defaults.popup_new_instance.accent_fg,
            ),
        },
        popup_import: PopupImportTheme {
            bg: rc(&raw.popup_import.bg, defaults.popup_import.bg),
            border_fg: rc(&raw.popup_import.border_fg, defaults.popup_import.border_fg),
            text_fg: rc(&raw.popup_import.text_fg, defaults.popup_import.text_fg),
            label_fg: rc(&raw.popup_import.label_fg, defaults.popup_import.label_fg),
            accent_fg: rc(&raw.popup_import.accent_fg, defaults.popup_import.accent_fg),
            cursor_fg: rc(&raw.popup_import.cursor_fg, defaults.popup_import.cursor_fg),
            placeholder_fg: rc(
                &raw.popup_import.placeholder_fg,
                defaults.popup_import.placeholder_fg,
            ),
        },
    }
}

fn default_theme_general() -> GeneralTheme {
    GeneralTheme::default()
}

fn default_theme_profiles() -> ProfilesTheme {
    ProfilesTheme::default()
}

fn default_theme_content() -> ContentTheme {
    ContentTheme::default()
}

fn default_theme_content_list() -> ContentListTheme {
    ContentListTheme::default()
}

fn default_theme_details() -> DetailsTheme {
    DetailsTheme::default()
}

fn default_theme_logs() -> LogsTheme {
    LogsTheme::default()
}

fn default_theme_log_overlay() -> LogOverlayTheme {
    LogOverlayTheme::default()
}

fn default_theme_status() -> StatusTheme {
    StatusTheme::default()
}

fn default_theme_search() -> SearchTheme {
    SearchTheme::default()
}

fn default_theme_screenshots() -> ScreenshotsTheme {
    ScreenshotsTheme::default()
}

fn default_theme_account() -> AccountTheme {
    AccountTheme::default()
}

fn default_theme_popup() -> PopupTheme {
    PopupTheme::default()
}

fn default_theme_popup_confirm() -> PopupConfirmTheme {
    PopupConfirmTheme::default()
}

fn default_theme_popup_error() -> PopupErrorTheme {
    PopupErrorTheme::default()
}

fn default_theme_popup_new_instance() -> PopupNewInstanceTheme {
    PopupNewInstanceTheme::default()
}

fn default_theme_popup_import() -> PopupImportTheme {
    PopupImportTheme::default()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Theme {
    #[serde(default = "default_theme_general")]
    pub general: GeneralTheme,
    #[serde(default = "default_theme_profiles")]
    pub profiles: ProfilesTheme,
    #[serde(default = "default_theme_content")]
    pub content: ContentTheme,
    #[serde(default = "default_theme_content_list")]
    pub content_list: ContentListTheme,
    #[serde(default = "default_theme_details")]
    pub details: DetailsTheme,
    #[serde(default = "default_theme_logs")]
    pub logs: LogsTheme,
    #[serde(default = "default_theme_log_overlay")]
    pub log_overlay: LogOverlayTheme,
    #[serde(default = "default_theme_status")]
    pub status: StatusTheme,
    #[serde(default = "default_theme_search")]
    pub search: SearchTheme,
    #[serde(default = "default_theme_screenshots")]
    pub screenshots: ScreenshotsTheme,
    #[serde(default = "default_theme_account")]
    pub account: AccountTheme,
    #[serde(default = "default_theme_popup")]
    pub popup: PopupTheme,
    #[serde(default = "default_theme_popup_confirm")]
    pub popup_confirm: PopupConfirmTheme,
    #[serde(default = "default_theme_popup_error")]
    pub popup_error: PopupErrorTheme,
    #[serde(default = "default_theme_popup_new_instance")]
    pub popup_new_instance: PopupNewInstanceTheme,
    #[serde(default = "default_theme_popup_import")]
    pub popup_import: PopupImportTheme,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            general: default_theme_general(),
            profiles: default_theme_profiles(),
            content: default_theme_content(),
            content_list: default_theme_content_list(),
            details: default_theme_details(),
            logs: default_theme_logs(),
            log_overlay: default_theme_log_overlay(),
            status: default_theme_status(),
            search: default_theme_search(),
            screenshots: default_theme_screenshots(),
            account: default_theme_account(),
            popup: default_theme_popup(),
            popup_confirm: default_theme_popup_confirm(),
            popup_error: default_theme_popup_error(),
            popup_new_instance: default_theme_popup_new_instance(),
            popup_import: default_theme_popup_import(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_theme(toml_str: &str) -> Theme {
        let raw: RawTheme = toml::from_str(toml_str).expect("theme TOML must parse");
        resolve_theme(raw)
    }

    #[test]
    fn test_theme_default_consistency() {
        let raw: RawTheme =
            toml::from_str(include_str!("../../assets/theme.toml")).expect("theme.toml must parse");
        let from_toml = resolve_theme(raw);
        let default = Theme::default();

        assert_eq!(from_toml.general.fg, default.general.fg);
        assert_eq!(
            from_toml.profiles.loader_fabric,
            default.profiles.loader_fabric
        );
        assert_eq!(from_toml.general.border_type, default.general.border_type);
    }

    #[test]
    fn test_partial_override() {
        let toml_str = r#"[general]
fg = "red"
"#;
        let theme = parse_theme(toml_str);

        assert_eq!(theme.general.fg, ratatui::style::Color::Red);
        assert_eq!(theme.general.bg, Theme::default().general.bg);
        assert_eq!(
            theme.profiles.loader_fabric,
            Theme::default().profiles.loader_fabric
        );
    }

    #[test]
    fn test_empty_toml_gives_defaults() {
        let theme = parse_theme("");
        assert_eq!(theme, Theme::default());
    }

    #[test]
    fn test_load_theme_from_missing_path() {
        let theme = load_theme_from_path(std::path::Path::new("/nonexistent/path/theme.toml"));
        assert_eq!(theme, Theme::default());
    }

    #[test]
    fn test_load_theme_from_valid_file() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theme.toml");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "[general]").unwrap();
        writeln!(file, r#"fg = "red""#).unwrap();
        drop(file);
        let theme = load_theme_from_path(&path);
        assert_eq!(theme.general.fg, ratatui::style::Color::Red);
        assert_eq!(theme.general.bg, Theme::default().general.bg);
    }

    #[test]
    fn test_ensure_theme_exists_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theme.toml");
        assert!(!path.exists());
        ensure_theme_exists(&path);
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        let _: RawTheme = toml::from_str(&content).expect("generated theme.toml must be valid");
    }

    #[test]
    fn test_ensure_theme_exists_no_overwrite() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theme.toml");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "[general]").unwrap();
        writeln!(file, r#"fg = "magenta""#).unwrap();
        drop(file);
        ensure_theme_exists(&path);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(
            content.contains("magenta"),
            "existing theme.toml should not be overwritten"
        );
    }

    #[test]
    fn test_border_style_deserialization() {
        let theme = parse_theme("[general]\nborder_type = \"plain\"\n");
        assert_eq!(theme.general.border_type, BorderStyle::Plain);

        let theme2 = parse_theme("[general]\nborder_type = \"double\"\n");
        assert_eq!(theme2.general.border_type, BorderStyle::Double);

        let theme3 = parse_theme("[general]\nborder_type = \"thick\"\n");
        assert_eq!(theme3.general.border_type, BorderStyle::Thick);

        let theme4 = parse_theme("");
        assert_eq!(theme4.general.border_type, BorderStyle::Rounded);
    }

    #[test]
    fn test_modifier_booleans() {
        let toml_str = "[profiles]\nselected_bold = false\n";
        let theme = parse_theme(toml_str);
        assert!(!theme.profiles.selected_bold);

        let default = Theme::default();
        assert!(default.profiles.selected_bold);
    }

    #[test]
    fn test_palette_resolution() {
        let theme = parse_theme(
            r##"[palette]
primary = "#123456"

[general]
fg = "primary"
bg = "#654321"
"##,
        );

        assert_eq!(theme.general.fg, Color::Rgb(0x12, 0x34, 0x56));
        assert_eq!(theme.general.bg, Color::Rgb(0x65, 0x43, 0x21));
    }

    #[test]
    fn test_invalid_toml_load_from_path() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "[general").unwrap();
        drop(file);
        let theme = load_theme_from_path(&path);
        assert_eq!(theme, Theme::default());
    }
}
