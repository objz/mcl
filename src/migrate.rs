//! Legacy mcl→rmcl path migration. Runs once before any other init in main().
//! Idempotent: absence of the old dir is the sentinel.

use std::fs;
use std::io;
use std::path::Path;

const OLD_NAME: &str = "mcl";
const NEW_NAME: &str = "rmcl";

pub fn run_legacy_rename() {
    if let Some(dir) = dirs_next::config_dir() {
        rename_top_level(&dir.join(OLD_NAME), &dir.join(NEW_NAME));
    }
    if let Some(dir) = dirs_next::data_dir() {
        let new_data = dir.join(NEW_NAME);
        rename_top_level(&dir.join(OLD_NAME), &new_data);
        cleanup_instance_leftovers(&new_data.join("instances"));
        rewrite_linux_desktop_entries(&dir, &new_data.join("instances"));
    }
    if let Some(dir) = dirs_next::cache_dir() {
        rename_top_level(&dir.join(OLD_NAME), &dir.join(NEW_NAME));
    }
    if let (Some(desk), Some(data)) = (dirs::desktop_dir(), dirs_next::data_dir()) {
        rewrite_native_desktop_shortcuts(&desk, &data.join(NEW_NAME).join("instances"));
    }
}

fn rename_top_level(old: &Path, new: &Path) {
    if !old.exists() {
        return;
    }
    if new.exists() {
        eprintln!(
            "rmcl migration: both {} and {} exist; leaving as-is, please merge manually",
            old.display(),
            new.display()
        );
        return;
    }
    match fs::rename(old, new) {
        Ok(_) => eprintln!(
            "rmcl migration: moved {} -> {}",
            old.display(),
            new.display()
        ),
        Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {
            if let Err(e2) = copy_dir_recursive(old, new) {
                eprintln!(
                    "rmcl migration: failed cross-device copy {} -> {}: {}",
                    old.display(),
                    new.display(),
                    e2
                );
                return;
            }
            if let Err(e3) = fs::remove_dir_all(old) {
                eprintln!(
                    "rmcl migration: copied but failed to remove {}: {}",
                    old.display(),
                    e3
                );
                return;
            }
            eprintln!(
                "rmcl migration: cross-device moved {} -> {}",
                old.display(),
                new.display()
            );
        }
        Err(e) => eprintln!(
            "rmcl migration: failed to rename {} -> {}: {}",
            old.display(),
            new.display(),
            e
        ),
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path)?;
        } else {
            fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

fn cleanup_instance_leftovers(instances_dir: &Path) {
    let Ok(entries) = fs::read_dir(instances_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let mc = entry.path().join(crate::storage::MINECRAFT_DIR_NAME);
        for leftover in [".mcl-shim.jar", ".mcl-log4j2.xml"] {
            let p = mc.join(leftover);
            if p.exists() {
                let _ = fs::remove_file(&p);
            }
        }
    }
}

fn rewrite_linux_desktop_entries(_data_dir: &Path, _instances_dir: &Path) {
    #[cfg(target_os = "linux")]
    {
        let apps_dir = _data_dir.join("applications");
        let Ok(entries) = fs::read_dir(_instances_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let sanitized = sanitize(&name.to_string_lossy());
            let old = apps_dir.join(format!("mcl-{sanitized}.desktop"));
            let new = apps_dir.join(format!("rmcl-{sanitized}.desktop"));
            if old.exists()
                && !new.exists()
                && let Ok(content) = fs::read_to_string(&old)
            {
                let new_content = content.replace("Exec=mcl ", "Exec=rmcl ");
                if fs::write(&new, new_content).is_ok() {
                    let _ = fs::remove_file(&old);
                }
            }
        }
    }
}

fn rewrite_native_desktop_shortcuts(_desktop_dir: &Path, _instances_dir: &Path) {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let ext = if cfg!(target_os = "windows") {
            "bat"
        } else {
            "command"
        };
        let Ok(entries) = fs::read_dir(_instances_dir) else {
            return;
        };
        for entry in entries.flatten() {
            let display = entry.file_name().to_string_lossy().into_owned();
            let path = _desktop_dir.join(format!("Minecraft - {display}.{ext}"));
            if !path.exists() {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path) {
                let new_content = content.replace("mcl instance launch", "rmcl instance launch");
                if new_content != content {
                    let _ = fs::write(&path, new_content);
                }
            }
        }
    }
}

#[allow(dead_code)]
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
    fn rename_top_level_moves_when_only_old_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("mcl");
        let new = tmp.path().join("rmcl");
        fs::create_dir_all(old.join("sub")).unwrap();
        fs::write(old.join("sub").join("f.txt"), b"hi").unwrap();

        rename_top_level(&old, &new);

        assert!(!old.exists());
        assert!(new.exists());
        assert_eq!(fs::read(new.join("sub").join("f.txt")).unwrap(), b"hi");
    }

    #[test]
    fn rename_top_level_skips_when_only_new_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("mcl");
        let new = tmp.path().join("rmcl");
        fs::create_dir_all(&new).unwrap();
        fs::write(new.join("marker.txt"), b"keep").unwrap();

        rename_top_level(&old, &new);

        assert!(!old.exists());
        assert_eq!(fs::read(new.join("marker.txt")).unwrap(), b"keep");
    }

    #[test]
    fn rename_top_level_skips_when_both_exist() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("mcl");
        let new = tmp.path().join("rmcl");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::write(old.join("a"), b"old").unwrap();
        fs::write(new.join("b"), b"new").unwrap();

        rename_top_level(&old, &new);

        assert!(old.exists(), "old should remain when both exist");
        assert!(new.exists(), "new should remain when both exist");
        assert_eq!(fs::read(old.join("a")).unwrap(), b"old");
        assert_eq!(fs::read(new.join("b")).unwrap(), b"new");
    }

    #[test]
    fn rename_top_level_noop_when_neither_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("mcl");
        let new = tmp.path().join("rmcl");

        rename_top_level(&old, &new);

        assert!(!old.exists());
        assert!(!new.exists());
    }

    #[test]
    fn cleanup_instance_leftovers_removes_shim_and_log4j() {
        let tmp = tempfile::tempdir().unwrap();
        let instances = tmp.path().join("instances");
        let mc = instances
            .join("Test")
            .join(crate::storage::MINECRAFT_DIR_NAME);
        fs::create_dir_all(&mc).unwrap();
        fs::write(mc.join(".mcl-shim.jar"), b"jar").unwrap();
        fs::write(mc.join(".mcl-log4j2.xml"), b"xml").unwrap();
        fs::write(mc.join("keep.txt"), b"keep").unwrap();

        cleanup_instance_leftovers(&instances);

        assert!(!mc.join(".mcl-shim.jar").exists());
        assert!(!mc.join(".mcl-log4j2.xml").exists());
        assert!(mc.join("keep.txt").exists());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn rewrite_linux_desktop_entries_renames_and_rewrites_exec() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        let instances = data.join("rmcl").join("instances");
        let apps = data.join("applications");
        fs::create_dir_all(instances.join("MyPack")).unwrap();
        fs::create_dir_all(&apps).unwrap();
        let old_entry = apps.join("mcl-MyPack.desktop");
        fs::write(
            &old_entry,
            "[Desktop Entry]\nName=Test\nExec=mcl instance launch \"MyPack\"\n",
        )
        .unwrap();

        rewrite_linux_desktop_entries(data, &instances);

        let new_entry = apps.join("rmcl-MyPack.desktop");
        assert!(!old_entry.exists(), "old .desktop should be removed");
        assert!(new_entry.exists(), "new .desktop should exist");
        let content = fs::read_to_string(&new_entry).unwrap();
        assert!(content.contains("Exec=rmcl instance launch \"MyPack\""));
    }

    #[test]
    fn copy_dir_recursive_copies_nested_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("a");
        let dst = tmp.path().join("b");
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("top.txt"), b"top").unwrap();
        fs::write(src.join("nested").join("inner.txt"), b"inner").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert_eq!(fs::read(dst.join("top.txt")).unwrap(), b"top");
        assert_eq!(
            fs::read(dst.join("nested").join("inner.txt")).unwrap(),
            b"inner"
        );
    }
}
