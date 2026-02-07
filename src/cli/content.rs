use std::io;
use std::path::Path;

use clap::ArgMatches;

use crate::cli::output::print_table;
use crate::instance::ModEntry;

type CliResult = Result<(), Box<dyn std::error::Error>>;
type Scanner = fn(&Path, &str) -> Vec<ModEntry>;

pub async fn handle_mod(matches: &ArgMatches) -> CliResult {
    handle_content(matches, "mod", crate::instance::mods::scan_mods)
}

pub async fn handle_pack(matches: &ArgMatches) -> CliResult {
    handle_content(matches, "pack", crate::instance::resource_packs::scan_resource_packs)
}

pub async fn handle_shader(matches: &ArgMatches) -> CliResult {
    handle_content(matches, "shader", crate::instance::shaders::scan_shaders)
}

fn handle_content(matches: &ArgMatches, kind: &str, scan: Scanner) -> CliResult {
    match matches.subcommand() {
        Some(("list", sub_matches)) => list_entries(required_arg(sub_matches, "instance")?, scan),
        Some(("enable", sub_matches)) => toggle_entry(
            required_arg(sub_matches, "instance")?,
            required_arg(sub_matches, kind)?,
            true,
            kind,
            scan,
        ),
        Some(("disable", sub_matches)) => toggle_entry(
            required_arg(sub_matches, "instance")?,
            required_arg(sub_matches, kind)?,
            false,
            kind,
            scan,
        ),
        _ => Ok(()),
    }
}

pub(crate) fn find_entry_by_stem<'a>(entries: &'a [ModEntry], target: &str) -> Option<&'a ModEntry> {
    entries
        .iter()
        .find(|entry| entry.file_stem.eq_ignore_ascii_case(target))
}

fn require_instance(instances_dir: &Path, name: &str) -> Result<(), io::Error> {
    if !instances_dir.join(name).join("instance.json").exists() {
        return Err(io::Error::other(format!("Instance '{}' not found", name)));
    }
    Ok(())
}

fn list_entries(instance: &str, scan: Scanner) -> CliResult {
    let instances_dir = crate::config::SETTINGS.paths.resolve_instances_dir();
    require_instance(&instances_dir, instance)?;
    let rows = scan(&instances_dir, instance)
        .into_iter()
        .map(|entry| {
            vec![
                entry.name,
                if entry.enabled {
                    "enabled".to_string()
                } else {
                    "disabled".to_string()
                },
            ]
        })
        .collect::<Vec<_>>();

    print_table(&["Name", "State"], &rows);
    Ok(())
}

fn toggle_entry(
    instance: &str,
    target: &str,
    should_enable: bool,
    kind: &str,
    scan: Scanner,
) -> CliResult {
    let instances_dir = crate::config::SETTINGS.paths.resolve_instances_dir();
    require_instance(&instances_dir, instance)?;
    let entries = scan(&instances_dir, instance);
    let entry = find_entry_by_stem(&entries, target).ok_or_else(|| {
        io::Error::other(format!("{} '{}' not found", kind, target))
    })?;

    if entry.enabled == should_enable {
        println!(
            "Already {}d.",
            if should_enable { "enable" } else { "disable" }
        );
        return Ok(());
    }

    crate::instance::mods::toggle_mod(entry)?;
    println!(
        "{}d '{}'.",
        if should_enable { "Enable" } else { "Disable" },
        entry.name
    );
    Ok(())
}

fn required_arg<'a>(matches: &'a ArgMatches, name: &str) -> Result<&'a str, io::Error> {
    matches
        .get_one::<String>(name)
        .map(String::as_str)
        .ok_or_else(|| io::Error::other(format!("missing required argument '{}'", name)))
}

#[cfg(test)]
mod tests {
    use super::find_entry_by_stem;
    use crate::instance::ModEntry;
    use std::path::PathBuf;

    fn entry(file_stem: &str) -> ModEntry {
        ModEntry {
            file_stem: file_stem.to_string(),
            name: file_stem.to_string(),
            description: String::new(),
            enabled: true,
            icon_bytes: None,
            path: PathBuf::from(file_stem),
            icon_lines: None,
        }
    }

    #[test]
    fn matches_by_stem_case_insensitively() {
        let entries = vec![entry("Sodium"), entry("Lithium")];
        let found = find_entry_by_stem(&entries, "sOdIuM").expect("entry should match");
        assert_eq!(found.file_stem, "Sodium");
    }

    #[test]
    fn returns_none_for_missing_stem() {
        let entries = vec![entry("Sodium")];
        assert!(find_entry_by_stem(&entries, "iris").is_none());
    }
}
