use std::path::{Path, PathBuf};

use crate::instance::models::InstanceConfig;

pub fn desktop_path(name: &str) -> Option<PathBuf> {
    dirs_next::data_dir().map(|d| {
        d.join("applications")
            .join(format!("mcl-{}.desktop", sanitize(name)))
    })
}

pub fn icon_path() -> Option<PathBuf> {
    dirs_next::data_dir().map(|d| d.join("mcl").join("icon.png"))
}

pub fn exists(name: &str) -> bool {
    desktop_path(name).map(|p| p.exists()).unwrap_or(false)
}

pub fn create(config: &InstanceConfig) -> std::io::Result<PathBuf> {
    let path = desktop_path(&config.name)
        .ok_or_else(|| std::io::Error::other("cannot resolve data directory"))?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let icon = icon_path().filter(|p| p.exists());
    let content = build_content(&config.name, icon.as_deref());
    std::fs::write(&path, content)?;
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
    fn build_content_with_icon() {
        let icon = PathBuf::from("/home/user/icon.png");
        let content = build_content("TestPack", Some(&icon));
        assert!(content.contains("Name=Minecraft - TestPack"));
        assert!(content.contains("Exec=mcl instance launch \"TestPack\""));
        assert!(content.contains("Icon=/home/user/icon.png"));
        assert!(content.contains("Terminal=true"));
        assert!(content.contains("Categories=Game;"));
    }

    #[test]
    fn build_content_without_icon() {
        let content = build_content("TestPack", None);
        assert!(content.contains("Name=Minecraft - TestPack"));
        assert!(!content.contains("Icon="));
    }
}
