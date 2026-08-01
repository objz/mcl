use super::*;
use crate::instance::content::entry::{ContentEntry, toggle_entry};
use std::path::PathBuf;

fn setup_mods_dir(tmp: &Path, instance: &str) -> PathBuf {
    let dir = tmp
        .join(instance)
        .join(crate::storage::MINECRAFT_DIR_NAME)
        .join("mods");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn scan_mods_empty_dir() {
    let tmp = tempfile::tempdir().unwrap();
    setup_mods_dir(tmp.path(), "inst");
    let mods = scan_mods(tmp.path(), "inst");
    assert!(mods.is_empty());
}

#[test]
fn scan_mods_missing_dir_returns_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let mods = scan_mods(tmp.path(), "ghost");
    assert!(mods.is_empty());
}

#[test]
fn scan_mods_enabled_disabled_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    std::fs::write(dir.join("active.jar"), b"PK\x03\x04").unwrap();
    std::fs::write(dir.join("inactive.jar.disabled"), b"PK\x03\x04").unwrap();
    let mods = scan_mods(tmp.path(), "inst");
    let active = mods.iter().find(|m| m.file_stem == "active").unwrap();
    let inactive = mods.iter().find(|m| m.file_stem == "inactive").unwrap();
    assert!(active.enabled);
    assert!(!inactive.enabled);
}

#[test]
fn scan_mods_ignores_non_jar() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    std::fs::write(dir.join("readme.txt"), "not a mod").unwrap();
    std::fs::write(dir.join("config.json"), "{}").unwrap();
    std::fs::write(dir.join("real.jar"), b"PK\x03\x04").unwrap();
    let mods = scan_mods(tmp.path(), "inst");
    assert_eq!(mods.len(), 1);
}

#[test]
fn scan_mods_sorted_case_insensitive() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    std::fs::write(dir.join("Zebra.jar"), b"PK\x03\x04").unwrap();
    std::fs::write(dir.join("alpha.jar"), b"PK\x03\x04").unwrap();
    std::fs::write(dir.join("Beta.jar"), b"PK\x03\x04").unwrap();
    let mods = scan_mods(tmp.path(), "inst");
    let names: Vec<&str> = mods.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, vec!["alpha", "Beta", "Zebra"]);
}

#[test]
fn toggle_entry_enable() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let disabled_path = dir.join("mymod.jar.disabled");
    std::fs::write(&disabled_path, b"PK\x03\x04").unwrap();

    let entry = ContentEntry {
        file_stem: "mymod".to_string(),
        name: "mymod".to_string(),
        source_slug: None,
        installed_path: None,
        provider_project: None,
        world_details: None,
        title_suffix: None,
        footer_label: None,
        description: String::new(),
        enabled: false,
        icon_bytes: None,
        provider_icon: false,
        provider_description: false,
        path: disabled_path.clone(),
        icon_lines: None,
    };

    toggle_entry(&entry).unwrap();
    assert!(!disabled_path.exists());
    assert!(dir.join("mymod.jar").exists());
}

use std::io::Write as _;

fn make_jar(dir: &Path, name: &str, entries: &[(&str, &[u8])]) {
    let path = dir.join(name);
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (entry_name, data) in entries {
        zip.start_file(*entry_name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn scan_mods_reads_fabric_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let meta = r#"{"name":"Fabric Mod","description":"A fabric mod","icon":"icon.png"}"#;
    make_jar(
        &dir,
        "fabric-mod.jar",
        &[("fabric.mod.json", meta.as_bytes())],
    );
    let mods = scan_mods(tmp.path(), "inst");
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].name, "Fabric Mod");
    assert_eq!(mods[0].description, "A fabric mod");
}

#[test]
fn scan_mods_reads_quilt_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let meta = r#"{"quilt_loader":{"metadata":{"name":"Quilt Mod","description":"A quilt mod"}}}"#;
    make_jar(
        &dir,
        "quilt-mod.jar",
        &[("quilt.mod.json", meta.as_bytes())],
    );
    let mods = scan_mods(tmp.path(), "inst");
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].name, "Quilt Mod");
    assert_eq!(mods[0].description, "A quilt mod");
}

#[test]
fn scan_mods_reads_forge_toml_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let meta = r#"
logoFile = "logo.png"

[[mods]]
displayName = "Forge Mod"
description = "A forge mod"
"#;
    make_jar(
        &dir,
        "forge-mod.jar",
        &[("META-INF/mods.toml", meta.as_bytes())],
    );
    let mods = scan_mods(tmp.path(), "inst");
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].name, "Forge Mod");
    assert_eq!(mods[0].description, "A forge mod");
}

#[test]
fn scan_mods_reads_neoforge_toml_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let meta = r#"
logoFile = "logo.png"

[[mods]]
displayName = "NeoForge Mod"
description = "A neoforge mod"
"#;
    make_jar(
        &dir,
        "neoforge-mod.jar",
        &[("META-INF/neoforge.mods.toml", meta.as_bytes())],
    );
    let mods = scan_mods(tmp.path(), "inst");
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].name, "NeoForge Mod");
    assert_eq!(mods[0].description, "A neoforge mod");
}

#[test]
fn scan_mods_reads_mcmod_info_array() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let meta = r#"[{"name":"Legacy Mod","description":"An old forge mod"}]"#;
    make_jar(&dir, "legacy-mod.jar", &[("mcmod.info", meta.as_bytes())]);
    let mods = scan_mods(tmp.path(), "inst");
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].name, "Legacy Mod");
    assert_eq!(mods[0].description, "An old forge mod");
}

#[test]
fn scan_mods_reads_mcmod_info_modlist() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let meta = r#"{"modList":[{"name":"Wrapped Mod","description":"Has modList wrapper"}]}"#;
    make_jar(&dir, "wrapped-mod.jar", &[("mcmod.info", meta.as_bytes())]);
    let mods = scan_mods(tmp.path(), "inst");
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].name, "Wrapped Mod");
    assert_eq!(mods[0].description, "Has modList wrapper");
}

#[test]
fn scan_mods_prefers_fabric_over_forge() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let fabric = r#"{"name":"Fabric Name","description":"fabric desc"}"#;
    let forge = "[[mods]]\ndisplayName = \"Forge Name\"\ndescription = \"forge desc\"\n";
    make_jar(
        &dir,
        "multi.jar",
        &[
            ("fabric.mod.json", fabric.as_bytes()),
            ("META-INF/mods.toml", forge.as_bytes()),
        ],
    );
    let mods = scan_mods(tmp.path(), "inst");
    assert_eq!(mods[0].name, "Fabric Name");
}

#[test]
fn scan_mods_prefers_quilt_over_forge() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let quilt = r#"{"quilt_loader":{"id":"x","version":"1","metadata":{"name":"Quilt Name","description":"quilt desc"}}}"#;
    let forge = "[[mods]]\ndisplayName = \"Forge Name\"\ndescription = \"forge desc\"\n";
    make_jar(
        &dir,
        "quilt-over-forge.jar",
        &[
            ("quilt.mod.json", quilt.as_bytes()),
            ("META-INF/mods.toml", forge.as_bytes()),
        ],
    );
    let mods = scan_mods(tmp.path(), "inst");
    assert_eq!(mods[0].name, "Quilt Name");
}

#[test]
fn scan_mods_prefers_forge_toml_over_mcmod_info() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let forge = "[[mods]]\ndisplayName = \"Forge Name\"\ndescription = \"forge desc\"\n";
    let mcmod = r#"[{"modid":"legacy","name":"Legacy Name","description":"legacy desc"}]"#;
    make_jar(
        &dir,
        "forge-over-legacy.jar",
        &[
            ("META-INF/mods.toml", forge.as_bytes()),
            ("mcmod.info", mcmod.as_bytes()),
        ],
    );
    let mods = scan_mods(tmp.path(), "inst");
    assert_eq!(mods[0].name, "Forge Name");
}

// when both neoforge.mods.toml and mods.toml exist in the same jar (the
// shape a dual-format mod might ship), the neoforge one wins because
// read_forge_toml_meta tries it first via .or_else.
#[test]
fn scan_mods_prefers_neoforge_toml_over_forge_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let neoforge = "[[mods]]\ndisplayName = \"NeoForge Name\"\ndescription = \"neoforge desc\"\n";
    let forge = "[[mods]]\ndisplayName = \"Forge Name\"\ndescription = \"forge desc\"\n";
    make_jar(
        &dir,
        "neoforge-over-forge.jar",
        &[
            ("META-INF/neoforge.mods.toml", neoforge.as_bytes()),
            ("META-INF/mods.toml", forge.as_bytes()),
        ],
    );
    let mods = scan_mods(tmp.path(), "inst");
    assert_eq!(mods[0].name, "NeoForge Name");
}

// a mods.toml with logoFile but no [[mods]] array (e.g. a dependency-only
// library jar) should still surface as a scanned mod with empty name +
// description but with the icon resolved.
#[test]
fn scan_mods_reads_mods_toml_without_mods_array() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let mods_toml = "logoFile = \"icon.png\"\n";
    let png_bytes = b"\x89PNG fake icon";
    make_jar(
        &dir,
        "lib-only.jar",
        &[
            ("META-INF/mods.toml", mods_toml.as_bytes()),
            ("icon.png", png_bytes),
        ],
    );
    let mods = scan_mods(tmp.path(), "inst");
    assert_eq!(mods.len(), 1);
    // file-stem fallback when metadata name is empty - covered here
    // because no other test exercises an empty-name + present-logo combo.
    assert_eq!(mods[0].name, "lib-only");
    assert_eq!(mods[0].icon_bytes.as_deref(), Some(png_bytes.as_slice()));
}

#[test]
fn scan_mods_fallback_icon_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let png_bytes = b"\x89PNG fake";
    make_jar(&dir, "no-meta.jar", &[("logo.png", png_bytes)]);
    let mods = scan_mods(tmp.path(), "inst");
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].name, "no-meta");
    assert_eq!(mods[0].icon_bytes.as_deref(), Some(png_bytes.as_slice()));
}

#[test]
fn icon_path_from_value_string() {
    let val = serde_json::json!("assets/icon.png");
    assert_eq!(icon_path_from_value(&val), "assets/icon.png");
}

#[test]
fn icon_path_from_value_map() {
    // serde_json::Map is a BTreeMap, so iteration is sorted by key.
    // "128" sorts before "64" lexicographically, so the first value wins.
    let val = serde_json::json!({"64": "icon_64.png", "128": "icon_128.png"});
    assert_eq!(icon_path_from_value(&val), "icon_128.png");
}

#[test]
fn icon_path_from_value_null() {
    let val = serde_json::Value::Null;
    assert_eq!(icon_path_from_value(&val), "");
}

#[test]
fn toggle_entry_disable() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = setup_mods_dir(tmp.path(), "inst");
    let enabled_path = dir.join("mymod.jar");
    std::fs::write(&enabled_path, b"PK\x03\x04").unwrap();

    let entry = ContentEntry {
        file_stem: "mymod".to_string(),
        name: "mymod".to_string(),
        source_slug: None,
        installed_path: None,
        provider_project: None,
        world_details: None,
        title_suffix: None,
        footer_label: None,
        description: String::new(),
        enabled: true,
        icon_bytes: None,
        provider_icon: false,
        provider_description: false,
        path: enabled_path.clone(),
        icon_lines: None,
    };

    toggle_entry(&entry).unwrap();
    assert!(!enabled_path.exists());
    assert!(dir.join("mymod.jar.disabled").exists());
}
