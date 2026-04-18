// creates OS-native shortcuts for launching instances directly:
// .desktop files on linux, .bat on windows, .command on macos

use std::path::{Path, PathBuf};

use crate::instance::models::InstanceConfig;

const ICON_BYTES: &[u8] = include_bytes!("../../assets/icon.svg");

pub fn desktop_path(name: &str) -> Option<PathBuf> {
    let sanitized = sanitize(name);

    #[cfg(target_os = "linux")]
    {
        dirs_next::data_dir().map(|d| {
            d.join("applications")
                .join(format!("mcl-{sanitized}.desktop"))
        })
    }

    #[cfg(target_os = "windows")]
    {
        dirs::desktop_dir().map(|d| d.join(format!("Minecraft - {sanitized}.bat")))
    }

    #[cfg(target_os = "macos")]
    {
        dirs::desktop_dir().map(|d| d.join(format!("Minecraft - {sanitized}.command")))
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = sanitized;
        None
    }
}

pub fn icon_path() -> Option<PathBuf> {
    dirs_next::data_dir().map(|d| d.join("mcl").join("icon.svg"))
}

// lazily writes the bundled svg icon to disk the first time a shortcut needs it
fn ensure_icon() -> Option<PathBuf> {
    let path = icon_path()?;
    if path.exists() {
        return Some(path);
    }
    let parent = path.parent()?;
    if let Err(e) = std::fs::create_dir_all(parent) {
        tracing::warn!("Failed to create icon directory: {}", e);
        return None;
    }
    if let Err(e) = std::fs::write(&path, ICON_BYTES) {
        tracing::warn!("Failed to write bundled icon: {}", e);
        return None;
    }
    Some(path)
}

pub fn exists(name: &str) -> bool {
    desktop_path(name).map(|p| p.exists()).unwrap_or(false)
}

pub fn create(config: &InstanceConfig) -> std::io::Result<PathBuf> {
    let path = desktop_path(&config.name)
        .ok_or_else(|| std::io::Error::other("cannot resolve shortcut directory"))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let icon = ensure_icon();
    let content = build_content(&config.name, icon.as_deref());
    std::fs::write(&path, content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(&path, perms)?;
    }

    Ok(path)
}

pub fn remove(name: &str) -> std::io::Result<()> {
    let Some(path) = desktop_path(name) else {
        return Ok(());
    };
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

pub fn toggle(config: &InstanceConfig) -> std::io::Result<bool> {
    if exists(&config.name) {
        remove(&config.name)?;
        Ok(false)
    } else {
        create(config)?;
        Ok(true)
    }
}

pub fn rename(old_name: &str, new_config: &InstanceConfig) -> std::io::Result<()> {
    if !exists(old_name) {
        return Ok(());
    }
    remove(old_name)?;
    create(new_config)?;
    Ok(())
}

fn build_content(name: &str, icon: Option<&Path>) -> String {
    #[cfg(target_os = "linux")]
    {
        build_linux_desktop(name, icon)
    }

    #[cfg(target_os = "windows")]
    {
        let _ = icon;
        build_windows_shortcut(name)
    }

    #[cfg(target_os = "macos")]
    {
        let _ = icon;
        build_macos_command(name)
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = (name, icon);
        String::new()
    }
}

#[cfg(target_os = "linux")]
fn build_linux_desktop(name: &str, icon: Option<&Path>) -> String {
    let mut out = String::new();
    out.push_str("[Desktop Entry]\n");
    out.push_str("Version=1.0\n");
    out.push_str("Type=Application\n");
    out.push_str(&format!("Name=Minecraft - {name}\n"));
    out.push_str(&format!("Comment=Launch {name} Minecraft instance\n"));
    out.push_str(&format!("Exec=mcl instance launch \"{name}\"\n"));
    if let Some(icon) = icon {
        out.push_str(&format!("Icon={}\n", icon.display()));
    }
    out.push_str("Terminal=true\n");
    out.push_str("Categories=Game;\n");
    out
}

#[cfg(target_os = "windows")]
fn build_windows_shortcut(name: &str) -> String {
    let mut out = String::new();
    out.push_str("@echo off\r\n");
    out.push_str(&format!("title Minecraft - {name}\r\n"));
    out.push_str(&format!("mcl instance launch \"{name}\"\r\n"));
    out.push_str("pause\r\n");
    out
}

#[cfg(target_os = "macos")]
fn build_macos_command(name: &str) -> String {
    let mut out = String::new();
    out.push_str("#!/bin/bash\n");
    out.push_str(&format!("# Launch Minecraft instance: {name}\n"));
    out.push_str(&format!("mcl instance launch \"{name}\"\n"));
    out
}

// replaces anything that isn't alphanumeric, dash, or underscore with _
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_alphanumeric() {
        assert_eq!(sanitize("my-instance_123"), "my-instance_123");
    }

    #[test]
    fn sanitize_replaces_special_chars() {
        assert_eq!(sanitize("my instance!"), "my_instance_");
        assert_eq!(sanitize("path/traversal"), "path_traversal");
    }

    #[test]
    fn desktop_path_returns_some() {
        let path = desktop_path("TestPack");
        if let Some(p) = path {
            let name = p.file_name().unwrap().to_str().unwrap();
            assert!(name.contains("TestPack"));
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn build_content_linux() {
        let content = build_content("TestPack", None);
        assert!(content.contains("Name=Minecraft - TestPack"));
        assert!(content.contains("Exec=mcl instance launch \"TestPack\""));
        assert!(content.contains("Terminal=true"));
        assert!(content.contains("Categories=Game;"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn build_content_linux_with_icon() {
        let icon = PathBuf::from("/tmp/icon.png");
        let content = build_content("TestPack", Some(&icon));
        assert!(content.contains("Icon=/tmp/icon.png"));
    }
}
