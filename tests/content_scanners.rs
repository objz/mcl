// integration tests for the public content-scanner APIs: shaders, worlds,
// and resource packs. each scanner walks an instance's minecraft subdir
// and returns ContentEntry rows; shape varies slightly per content type
// (worlds are directories only, packs and shaders accept .zip or dir, etc.).

use std::path::{Path, PathBuf};

use rmcl::instance::content::resource_packs::scan_resource_packs;
use rmcl::instance::content::shaders::scan_shaders;
use rmcl::instance::content::worlds::scan_worlds;

fn setup_subdir(tmp: &Path, instance: &str, sub: &str) -> PathBuf {
    let dir = tmp.join(instance).join("minecraft").join(sub);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// minimal zip header byte stream; enough that the scanner accepts the file
// without panicking even though it can't actually unpack metadata.
const ZIP_HEADER: &[u8] = b"PK\x03\x04";

#[test]
fn shaders_empty_dir_returns_empty() {
    let tmp = tempfile::tempdir().unwrap();
    setup_subdir(tmp.path(), "inst", "shaderpacks");
    assert!(scan_shaders(tmp.path(), "inst").is_empty());
}

#[test]
fn shaders_missing_dir_returns_empty() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(scan_shaders(tmp.path(), "ghost").is_empty());
}

#[test]
fn shaders_disabled_variants() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_subdir(tmp.path(), "inst", "shaderpacks");
    std::fs::write(dir.join("active.zip"), ZIP_HEADER).unwrap();
    std::fs::write(dir.join("off.zip.disabled"), ZIP_HEADER).unwrap();
    std::fs::create_dir(dir.join("dirshader.disabled")).unwrap();
    let shaders = scan_shaders(tmp.path(), "inst");
    let active = shaders.iter().find(|s| s.file_stem == "active").unwrap();
    let off = shaders.iter().find(|s| s.file_stem == "off").unwrap();
    let diroff = shaders.iter().find(|s| s.file_stem == "dirshader").unwrap();
    assert!(active.enabled);
    assert!(!off.enabled);
    assert!(!diroff.enabled);
}

#[test]
fn shaders_ignores_non_shader_files() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_subdir(tmp.path(), "inst", "shaderpacks");
    std::fs::write(dir.join("readme.txt"), "not a shader").unwrap();
    std::fs::write(dir.join("valid.zip"), ZIP_HEADER).unwrap();
    assert_eq!(scan_shaders(tmp.path(), "inst").len(), 1);
}

// worlds are always directories (no zip form), so the test shape differs.

#[test]
fn worlds_empty_dir_returns_empty() {
    let tmp = tempfile::tempdir().unwrap();
    setup_subdir(tmp.path(), "inst", "saves");
    assert!(scan_worlds(tmp.path(), "inst").is_empty());
}

#[test]
fn worlds_missing_dir_returns_empty() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(scan_worlds(tmp.path(), "ghost").is_empty());
}

#[test]
fn worlds_ignores_files() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_subdir(tmp.path(), "inst", "saves");
    std::fs::create_dir(dir.join("World1")).unwrap();
    std::fs::write(dir.join("stray-file.txt"), "not a world").unwrap();
    assert_eq!(scan_worlds(tmp.path(), "inst").len(), 1);
}

#[test]
fn worlds_disabled_world() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_subdir(tmp.path(), "inst", "saves");
    std::fs::create_dir(dir.join("ActiveWorld")).unwrap();
    std::fs::create_dir(dir.join("HiddenWorld.disabled")).unwrap();
    let worlds = scan_worlds(tmp.path(), "inst");
    let active = worlds
        .iter()
        .find(|w| w.file_stem == "ActiveWorld")
        .unwrap();
    let hidden = worlds
        .iter()
        .find(|w| w.file_stem == "HiddenWorld")
        .unwrap();
    assert!(active.enabled);
    assert!(!hidden.enabled);
}

#[test]
fn worlds_sorted_case_insensitive() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_subdir(tmp.path(), "inst", "saves");
    std::fs::create_dir(dir.join("Zeta")).unwrap();
    std::fs::create_dir(dir.join("alpha")).unwrap();
    std::fs::create_dir(dir.join("Beta")).unwrap();
    let worlds = scan_worlds(tmp.path(), "inst");
    let names: Vec<&str> = worlds.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "Beta", "Zeta"]);
}

#[test]
fn resource_packs_empty_dir_returns_empty() {
    let tmp = tempfile::tempdir().unwrap();
    setup_subdir(tmp.path(), "inst", "resourcepacks");
    assert!(scan_resource_packs(tmp.path(), "inst").is_empty());
}

#[test]
fn resource_packs_missing_dir_returns_empty() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(scan_resource_packs(tmp.path(), "ghost").is_empty());
}

#[test]
fn resource_packs_disabled_variants() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_subdir(tmp.path(), "inst", "resourcepacks");
    std::fs::write(dir.join("on.zip"), ZIP_HEADER).unwrap();
    std::fs::write(dir.join("off.zip.disabled"), ZIP_HEADER).unwrap();
    std::fs::create_dir(dir.join("diron")).unwrap();
    std::fs::create_dir(dir.join("diroff.disabled")).unwrap();
    let packs = scan_resource_packs(tmp.path(), "inst");
    assert_eq!(packs.len(), 4);
    let on_zip = packs.iter().find(|p| p.file_stem == "on").unwrap();
    let off_zip = packs.iter().find(|p| p.file_stem == "off").unwrap();
    let on_dir = packs.iter().find(|p| p.file_stem == "diron").unwrap();
    let off_dir = packs.iter().find(|p| p.file_stem == "diroff").unwrap();
    assert!(on_zip.enabled);
    assert!(!off_zip.enabled);
    assert!(on_dir.enabled);
    assert!(!off_dir.enabled);
}

#[test]
fn resource_packs_ignores_non_pack_files() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_subdir(tmp.path(), "inst", "resourcepacks");
    std::fs::write(dir.join("notes.txt"), "not a pack").unwrap();
    std::fs::write(dir.join("valid.zip"), ZIP_HEADER).unwrap();
    assert_eq!(scan_resource_packs(tmp.path(), "inst").len(), 1);
}
