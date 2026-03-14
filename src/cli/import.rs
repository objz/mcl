use clap::ArgMatches;

use crate::instance::InstanceManager;
use crate::net::modrinth;

type CliResult = Result<(), Box<dyn std::error::Error>>;

pub async fn handle_import(matches: &ArgMatches) -> CliResult {
    let input = matches.get_one::<String>("source").unwrap();
    let override_name = matches.get_one::<String>("name");
    let override_version = matches.get_one::<String>("version");

    let instances_dir = crate::config::SETTINGS.paths.resolve_instances_dir();
    let meta_dir = crate::config::SETTINGS.paths.resolve_meta_dir();
    let manager = InstanceManager::new(instances_dir, meta_dir);
    let client = crate::net::HttpClient::new();

    let parsed = modrinth::parse_input(input);

    let mrpack_path = match parsed {
        modrinth::ModrinthInput::LocalFile(ref path) => {
            let resolved = if let Some(stripped) = path.strip_prefix("~/") {
                dirs_next::home_dir()
                    .map(|h| h.join(stripped))
                    .unwrap_or_else(|| std::path::PathBuf::from(path))
            } else {
                std::path::PathBuf::from(path)
            };
            if !resolved.exists() {
                return Err(format!("File not found: {}", resolved.display()).into());
            }
            resolved
        }
        modrinth::ModrinthInput::ProjectSlug(ref slug) => {
            println!("Fetching project '{slug}'...");
            let project = modrinth::fetch_project(&client, slug).await
                .map_err(|e| format!("Failed to fetch project: {e}"))?;
            println!("Found: {}", project.title);

            let versions = modrinth::fetch_versions(&client, slug).await
                .map_err(|e| format!("Failed to fetch versions: {e}"))?;

            if versions.is_empty() {
                return Err("No versions found for this modpack".into());
            }

            let version = if let Some(version_name) = override_version {
                versions
                    .iter()
                    .find(|v| v.version_number == *version_name || v.name == *version_name)
                    .ok_or_else(|| format!("Version '{version_name}' not found"))?
            } else {
                &versions[0]
            };

            println!(
                "Using version {} ({})",
                version.version_number,
                version.game_versions.first().unwrap_or(&"?".to_string())
            );

            let tmp_dir = manager.meta_dir.join("tmp");
            std::fs::create_dir_all(&tmp_dir)?;
            modrinth::download_mrpack(&client, version, &tmp_dir).await
                .map_err(|e| format!("Failed to download .mrpack: {e}"))?
        }
        modrinth::ModrinthInput::VersionId { ref slug, ref version_id } => {
            println!("Fetching version '{version_id}'...");
            let _slug = slug;
            let version = modrinth::fetch_version(&client, version_id).await
                .map_err(|e| format!("Failed to fetch version: {e}"))?;

            println!(
                "Found: {} ({})",
                version.version_number,
                version.game_versions.first().unwrap_or(&"?".to_string())
            );

            let tmp_dir = manager.meta_dir.join("tmp");
            std::fs::create_dir_all(&tmp_dir)?;
            modrinth::download_mrpack(&client, &version, &tmp_dir).await
                .map_err(|e| format!("Failed to download .mrpack: {e}"))?
        }
    };

    let index = modrinth::parse_mrpack(&mrpack_path)
        .map_err(|e| format!("Failed to parse .mrpack: {e}"))?;

    let mut summary = crate::instance::import::build_summary(&index, mrpack_path)
        .map_err(|e| format!("Invalid modpack: {e}"))?;

    if let Some(name) = override_name {
        summary.name = name.clone();
    }

    println!(
        "Importing '{}' — {} {} ({} mods, {} overrides)",
        summary.name,
        summary.game_version,
        summary.loader,
        summary.mod_count,
        summary.override_count
    );

    let config = crate::instance::import::execute_import(&summary, &manager).await
        .map_err(|e| format!("Import failed: {e}"))?;

    println!("Instance '{}' created successfully.", config.name);
    Ok(())
}
