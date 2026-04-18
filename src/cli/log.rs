use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::ArgMatches;

use crate::cli::output::print_table;

type CliResult = Result<(), Box<dyn std::error::Error>>;

pub async fn handle_log(matches: &ArgMatches) -> CliResult {
    match matches.subcommand() {
        Some(("list", sub_matches)) => list_logs(required_arg(sub_matches, "instance")?),
        Some(("show", sub_matches)) => show_log(sub_matches).await,
        _ => Ok(()),
    }
}

fn list_logs(instance: &str) -> CliResult {
    let instances_dir = crate::config::SETTINGS.paths.resolve_instances_dir();
    require_instance(&instances_dir, instance)?;
    let rows = crate::instance::log_files::scan_log_files(&instances_dir, instance)
        .into_iter()
        .map(|entry| {
            let size = std::fs::metadata(&entry.path)
                .map(|meta| meta.len())
                .unwrap_or(0);
            vec![entry.name, size.to_string()]
        })
        .collect::<Vec<_>>();

    print_table(&["File", "Size"], &rows);
    Ok(())
}

async fn show_log(matches: &ArgMatches) -> CliResult {
    let instance = required_arg(matches, "instance")?;
    let file = matches.get_one::<String>("file").map(String::as_str);
    let follow = matches.get_flag("follow");
    let instances_dir = crate::config::SETTINGS.paths.resolve_instances_dir();
    require_instance(&instances_dir, instance)?;
    let path = resolve_log_path(&instances_dir, instance, file)?;

    let lines = crate::instance::log_files::read_log_file(&path);
    for line in &lines {
        println!("{}", line);
    }

    if follow {
        let mut last_len = lines.len();
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let new_lines = crate::instance::log_files::read_log_file(&path);
            for line in new_lines.iter().skip(last_len) {
                println!("{}", line);
            }
            last_len = new_lines.len();
        }
    }

    Ok(())
}

pub(crate) fn resolve_log_path(
    instances_dir: &Path,
    instance: &str,
    file: Option<&str>,
) -> Result<PathBuf, io::Error> {
    if let Some(name) = file {
        let path = crate::instance::log_files::log_dir(instances_dir, instance).join(name);
        if !path.exists() {
            return Err(io::Error::other(format!("log '{}' not found", name)));
        }
        return Ok(path);
    }

    crate::instance::log_files::scan_log_files(instances_dir, instance)
        .into_iter()
        .next()
        .map(|entry| entry.path)
        .ok_or_else(|| io::Error::other(format!("no log files found for '{}'", instance)))
}

use super::utils::{require_instance, required_arg};

#[cfg(test)]
mod tests {
    use super::resolve_log_path;

    fn unique_temp_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "mcl_cli_log_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should work")
                .as_nanos()
        ))
    }

    #[test]
    fn resolves_latest_log_when_no_file_is_given() {
        let root = unique_temp_dir();
        let dir = root.join("demo/.minecraft/logs/launches");
        std::fs::create_dir_all(&dir).expect("log directory should exist");
        std::fs::write(dir.join("2024-01-02_03-04-05.log"), "newer").expect("write newer log");
        std::fs::write(dir.join("2024-01-01_03-04-05.log"), "older").expect("write older log");

        let path = resolve_log_path(&root, "demo", None).expect("latest log should resolve");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("2024-01-02_03-04-05.log")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolves_named_log_file() {
        let root = unique_temp_dir();
        let dir = root.join("demo/.minecraft/logs/launches");
        std::fs::create_dir_all(&dir).expect("log directory should exist");
        std::fs::write(dir.join("latest.log"), "hello").expect("write named log");

        let path =
            resolve_log_path(&root, "demo", Some("latest.log")).expect("named log should resolve");
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("latest.log")
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
