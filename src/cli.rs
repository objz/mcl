use clap::{Arg, ArgAction, Command};
use crate::tui;

pub fn init() {
    let matches = Command::new("mcl")
        .about("Minecraft CLI Launcher")
        .version("1.0.0")
        .subcommand_required(false)
        .arg_required_else_help(false)
        .subcommand(
            Command::new("launch")
                .about("Launch Minecraft with a specific profile")
                .arg_required_else_help(true)
                .arg(
                    Arg::new("profile")
                        .short('p')
                        .long("profile")
                        .help("Profile to launch (e.g., main)")
                        .required(true)
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("offline")
                        .short('o')
                        .long("offline")
                        .help("Launch Minecraft in offline mode")
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("memory")
                        .short('m')
                        .long("memory")
                        .help("Set memory allocation for Minecraft (e.g., 4G, 512M)")
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("resolution")
                        .short('r')
                        .long("resolution")
                        .help("Set screen resolution (e.g., 1920x1080)")
                        .action(ArgAction::Set),
                )
                .arg(
                    Arg::new("jvm-args")
                        .short('j')
                        .long("jvm-args")
                        .help("Custom JVM arguments for Minecraft (e.g., -Xmx4G)")
                        .action(ArgAction::Set)
                        .num_args(1..),
                )
                .arg(
                    Arg::new("no-window")
                        .short('n')
                        .long("no-window")
                        .help("Run Minecraft in headless mode (no graphical window)")
                        .action(ArgAction::SetTrue),
                ),
        )
        .subcommand(
            Command::new("profiles")
                .about("Manage Minecraft profiles")
                .arg_required_else_help(true)
                .arg(
                    Arg::new("list")
                        .short('l')
                        .long("list")
                        .help("List all available profiles")
                        .conflicts_with("delete") // Ensures list and delete are mutually exclusive
                        .action(ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("delete")
                        .short('d')
                        .long("delete")
                        .help("Delete a specific profile")
                        .action(ArgAction::Set),
                ),
        )
        .get_matches();
    
    if matches.subcommand().is_none() {
        open_tui();
    }

    match matches.subcommand() {
        Some(("launch", launch_matches)) => {
            let profile = launch_matches
                .get_one::<String>("profile")
                .expect("Profile is required");

            let mem_default = String::from("Default");
            let memory = launch_matches
                .get_one::<String>("memory")
                .unwrap_or(&mem_default);

            let res_default = String::from("Default");
            let resolution = launch_matches
                .get_one::<String>("resolution")
                .unwrap_or(&res_default);

            let jvm_args = launch_matches
                .get_many::<String>("jvm-args")
                .map(|args| args.map(|s| s.as_str()).collect::<Vec<_>>().join(" "))
                .unwrap_or_else(|| "None".to_string());

            if launch_matches.get_flag("offline") {
                println!("Launching profile '{}' in offline mode...", profile);
            } else {
                println!("Launching profile '{}' in online mode...", profile);
            }

            println!("Memory: {}", memory);
            println!("Resolution: {}", resolution);
            println!("JVM Args: {}", jvm_args);

            if launch_matches.get_flag("no-window") {
                println!("Running in headless mode...");
            }
        }
        Some(("profiles", profiles_matches)) => {
            if profiles_matches.get_flag("list") {
                println!("Listing all profiles...");
            } else if let Some(profile) = profiles_matches.get_one::<String>("delete") {
                println!("Deleting profile '{}'...", profile);
            }
        }
        _ => {},
    }

    fn open_tui() {
       tui::show().unwrap(); 
    }
}
