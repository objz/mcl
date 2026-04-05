use std::io::{self, Write};
use std::time::Duration;

use clap::ArgMatches;

use crate::cli::output::{format_datetime, print_table};
use crate::instance::{InstanceManager, ModLoader};
use crate::running::RunState;

type CliResult = Result<(), Box<dyn std::error::Error>>;

pub async fn handle_instance(matches: &ArgMatches) -> CliResult {
    match matches.subcommand() {
        Some(("list", _)) => list_instances(),
        Some(("create", sub_matches)) => create_instance(sub_matches).await,
        Some(("delete", sub_matches)) => delete_instance(sub_matches),
        Some(("rename", sub_matches)) => rename_instance(sub_matches),
        Some(("launch", sub_matches)) => launch_instance(sub_matches).await,
        Some(("config", sub_matches)) => config_instance(sub_matches),
        Some(("desktop", sub_matches)) => desktop_instance(sub_matches),
        _ => Ok(()),
    }
}

pub(crate) fn parse_loader(input: &str) -> Result<ModLoader, String> {
    match input.to_lowercase().as_str() {
        "vanilla" => Ok(ModLoader::Vanilla),
        "fabric" => Ok(ModLoader::Fabric),
        "forge" => Ok(ModLoader::Forge),
        "neoforge" => Ok(ModLoader::NeoForge),
        "quilt" => Ok(ModLoader::Quilt),
        _ => Err(format!(
            "unknown loader '{}'. Valid: vanilla, fabric, forge, neoforge, quilt",
            input
        )),
    }
}

pub(crate) fn parse_resolution(input: &str) -> Result<(u32, u32), String> {
    let (width, height) = input
        .split_once('x')
        .ok_or_else(|| "resolution must be in WxH format".to_string())?;
    let width = width
        .parse::<u32>()
        .map_err(|_| "resolution width must be a positive integer".to_string())?;
    let height = height
        .parse::<u32>()
        .map_err(|_| "resolution height must be a positive integer".to_string())?;

    if width == 0 || height == 0 {
        return Err("resolution values must be greater than zero".to_string());
    }

    Ok((width, height))
}

fn manager() -> InstanceManager {
    let instances_dir = crate::config::SETTINGS.paths.resolve_instances_dir();
    let meta_dir = crate::config::SETTINGS.paths.resolve_meta_dir();
    InstanceManager::new(instances_dir, meta_dir)
}

fn list_instances() -> CliResult {
    let mut instances = manager().load_all();
    instances.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));

    let rows = instances
        .into_iter()
        .map(|config| {
            vec![
                config.name,
                config.game_version,
                config.loader.to_string(),
                config
                    .last_played
                    .as_ref()
                    .map(format_datetime)
                    .unwrap_or_else(|| "-".to_string()),
            ]
        })
        .collect::<Vec<_>>();

    print_table(&["Name", "Version", "Loader", "Last Played"], &rows);
    Ok(())
}

async fn create_instance(matches: &ArgMatches) -> CliResult {
    let name = required_arg(matches, "name")?;
    let version = required_arg(matches, "version")?;
    let loader = parse_loader(required_arg(matches, "loader")?)
        .map_err(io::Error::other)?;
    let loader_version = matches.get_one::<String>("loader-version").map(String::as_str);
    let manager = manager();

    println!("Creating instance '{}'...", name);
    let config = manager.create(name, version, loader, loader_version).await?;
    println!(
        "Created '{}'  ({} {})",
        config.name, config.game_version, config.loader
    );
    Ok(())
}

fn delete_instance(matches: &ArgMatches) -> CliResult {
    let name = required_arg(matches, "name")?;
    if !matches.get_flag("yes") && !confirm(&format!("Delete '{}'", name))? {
        println!("Cancelled.");
        return Ok(());
    }

    manager().delete(name)?;
    println!("Deleted '{}'.", name);
    Ok(())
}

fn rename_instance(matches: &ArgMatches) -> CliResult {
    let old_name = required_arg(matches, "old")?;
    let new_name = required_arg(matches, "new")?;
    manager().rename(old_name, new_name)?;
    println!("Renamed '{}' to '{}'.", old_name, new_name);
    Ok(())
}

async fn launch_instance(matches: &ArgMatches) -> CliResult {
    let name = required_arg(matches, "name")?;
    let manager = manager();
    let instances_dir = crate::config::SETTINGS.paths.resolve_instances_dir();
    let meta_dir = crate::config::SETTINGS.paths.resolve_meta_dir();
    let config = manager.load_one(name)?;

    println!("Launching '{}'...", name);
    crate::instance::launch::launch(&config, &instances_dir, &meta_dir)
        .await
        .map_err(|error| io::Error::other(format!("Launch failed: {}", error)))?;

    loop {
        match crate::running::get(name) {
            Some(RunState::Crashed(Some(code))) => {
                println!("Game exited with status {}.", code);
                break;
            }
            Some(RunState::Crashed(None)) => {
                println!("Game exited with status terminated.");
                break;
            }
            Some(_) => tokio::time::sleep(Duration::from_millis(500)).await,
            None => {
                println!("Game exited with status 0.");
                break;
            }
        }
    }

    Ok(())
}

fn desktop_instance(matches: &ArgMatches) -> CliResult {
    let name = required_arg(matches, "name")?;
    let config = manager().load_one(name)?;
    let enabled = crate::instance::desktop::toggle(&config)?;
    if enabled {
        println!("Desktop shortcut created for '{}'.", name);
    } else {
        println!("Desktop shortcut removed for '{}'.", name);
    }
    Ok(())
}

fn config_instance(matches: &ArgMatches) -> CliResult {
    let name = required_arg(matches, "name")?;
    let manager = manager();

    if let Some(set_value) = matches.get_one::<String>("set") {
        let (key, value) = set_value
            .split_once('=')
            .ok_or_else(|| io::Error::other("--set must be in key=value format"))?;

        let mut config = manager.load_one(name)?;
        apply_config_update(&mut config, key, value)?;
        manager.save(&config)?;
        println!("Updated '{}' {}.", name, key);
        return Ok(());
    }

    let config = manager.load_one(name)?;
    let rows = vec![
        vec!["name".to_string(), config.name],
        vec!["game-version".to_string(), config.game_version],
        vec!["loader".to_string(), config.loader.to_string()],
        vec![
            "loader-version".to_string(),
            config.loader_version.unwrap_or_else(|| "-".to_string()),
        ],
        vec!["created".to_string(), format_datetime(&config.created)],
        vec![
            "last-played".to_string(),
            config
                .last_played
                .as_ref()
                .map(format_datetime)
                .unwrap_or_else(|| "-".to_string()),
        ],
        vec![
            "java-path".to_string(),
            config.java_path.unwrap_or_else(|| "-".to_string()),
        ],
        vec![
            "memory-max".to_string(),
            config.memory_max.unwrap_or_else(|| "-".to_string()),
        ],
        vec![
            "memory-min".to_string(),
            config.memory_min.unwrap_or_else(|| "-".to_string()),
        ],
        vec![
            "jvm-args".to_string(),
            if config.jvm_args.is_empty() {
                "-".to_string()
            } else {
                config.jvm_args.join(" ")
            },
        ],
        vec![
            "resolution".to_string(),
            config
                .resolution
                .map(|(width, height)| format!("{}x{}", width, height))
                .unwrap_or_else(|| "-".to_string()),
        ],
    ];

    print_table(&["Field", "Value"], &rows);
    Ok(())
}

fn apply_config_update(
    config: &mut crate::instance::InstanceConfig,
    key: &str,
    value: &str,
) -> CliResult {
    match key {
        "memory-max" => {
            config.memory_max = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        }
        "memory-min" => {
            config.memory_min = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        }
        "java-path" => {
            config.java_path = if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        }
        "jvm-args" => {
            config.jvm_args = value.split_whitespace().map(String::from).collect();
        }
        "resolution" => {
            config.resolution = if value.is_empty() {
                None
            } else {
                Some(parse_resolution(value).map_err(io::Error::other)?)
            };
        }
        _ => {
            return Err(io::Error::other(format!("unknown key '{}'", key)).into());
        }
    }

    Ok(())
}

fn confirm(message: &str) -> Result<bool, io::Error> {
    print!("{}? [y/N] ", message);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

fn required_arg<'a>(matches: &'a ArgMatches, name: &str) -> Result<&'a str, io::Error> {
    matches
        .get_one::<String>(name)
        .map(String::as_str)
        .ok_or_else(|| io::Error::other(format!("missing required argument '{}'", name)))
}

#[cfg(test)]
mod tests {
    use super::parse_resolution;

    #[test]
    fn parses_valid_resolution() {
        assert_eq!(parse_resolution("1920x1080").expect("should parse"), (1920, 1080));
    }

    #[test]
    fn rejects_invalid_resolution_format() {
        assert!(parse_resolution("1920").is_err());
        assert!(parse_resolution("1920xa").is_err());
        assert!(parse_resolution("0x1080").is_err());
    }
}
