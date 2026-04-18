// shared CLI helpers: confirmation prompts, arg extraction, instance validation
use std::io::{self, Write};

use clap::ArgMatches;

pub fn confirm(message: &str) -> Result<bool, io::Error> {
    print!("{}? [y/N] ", message);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

pub fn required_arg<'a>(matches: &'a ArgMatches, name: &str) -> Result<&'a str, io::Error> {
    matches
        .get_one::<String>(name)
        .map(String::as_str)
        .ok_or_else(|| io::Error::other(format!("missing required argument '{name}'")))
}

// checks for instance.json rather than just the directory, since a folder
// without config is just a sad empty directory pretending to be an instance
pub fn require_instance(instances_dir: &std::path::Path, name: &str) -> Result<(), io::Error> {
    if !instances_dir.join(name).join("instance.json").exists() {
        return Err(io::Error::other(format!("Instance '{name}' not found")));
    }
    Ok(())
}
