// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

fn version(name: &str, number: &str) -> modrinth::VersionInfo {
    modrinth::VersionInfo {
        id: number.to_owned(),
        project_id: "project".to_owned(),
        name: name.to_owned(),
        version_number: number.to_owned(),
        game_versions: vec!["1.21.1".to_owned()],
        loaders: vec!["fabric".to_owned()],
        version_type: modrinth::VersionType::Release,
        dependencies: Vec::new(),
        date_published: String::new(),
        files: Vec::new(),
    }
}

#[test]
fn version_override_matches_name_or_number_and_rejects_unknown_values() {
    let versions = vec![version("Stable", "1.0.0"), version("Beta", "2.0.0-beta")];

    assert_eq!(select_version(&versions, None).unwrap().name, "Stable");
    assert_eq!(
        select_version(&versions, Some("Beta"))
            .unwrap()
            .version_number,
        "2.0.0-beta"
    );
    assert_eq!(
        select_version(&versions, Some("1.0.0")).unwrap().name,
        "Stable"
    );
    assert_eq!(
        select_version(&versions, Some("missing")).unwrap_err(),
        "Version 'missing' not found"
    );
}

#[test]
fn import_command_parses_name_and_version_overrides() {
    let matches = crate::cli::build_command()
        .try_get_matches_from([
            "rmcl",
            "import",
            "example-pack",
            "--name",
            "Example",
            "--version",
            "1.2.3",
        ])
        .unwrap();
    let import = matches.subcommand_matches("import").unwrap();

    assert_eq!(
        import.get_one::<String>("source").map(String::as_str),
        Some("example-pack")
    );
    assert_eq!(
        import.get_one::<String>("name").map(String::as_str),
        Some("Example")
    );
    assert_eq!(
        import.get_one::<String>("version").map(String::as_str),
        Some("1.2.3")
    );
}

#[tokio::test]
async fn missing_local_pack_returns_a_file_not_found_error() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing.mrpack");
    let source = missing.to_string_lossy().into_owned();
    let matches = crate::cli::build_command()
        .try_get_matches_from(["rmcl", "import", &source])
        .unwrap();
    let import = matches.subcommand_matches("import").unwrap();

    let error = handle_import(import).await.unwrap_err().to_string();

    assert_eq!(error, format!("File not found: {}", missing.display()));
}
