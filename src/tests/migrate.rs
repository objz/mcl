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
