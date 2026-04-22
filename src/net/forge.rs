// forge mod loader: version discovery via promotions API, download and
// installation. modern forge runs a java installer, old forge (pre-1.13ish)
// doesn't support headless install so we extract directly from the jar.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::instance::loader::GameVersion;
use crate::net::{HttpClient, NetError, download_file};
use crate::tui::progress::{set_action, set_sub_action};

const FORGE_PROMOTIONS_URL: &str =
    "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
const FORGE_MAVEN_BASE: &str = "https://maven.minecraftforge.net/net/minecraftforge/forge";

#[derive(Debug, Deserialize)]
struct ForgePromotions {
    promos: HashMap<String, String>,
}

// forge promotions use keys like "1.20.1-recommended", "1.20.1-latest"
// so this filters by game version prefix and extracts the forge version values
pub async fn fetch_forge_versions(
    client: &HttpClient,
    game_version: &str,
) -> Result<Vec<String>, NetError> {
    let promotions: ForgePromotions = client.get_json(FORGE_PROMOTIONS_URL).await?;

    let prefix = format!("{}-", game_version);
    let mut versions: Vec<String> = promotions
        .promos
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .map(|(_, value)| value.clone())
        .collect();

    versions.sort();
    versions.dedup();
    Ok(versions)
}

// extracts unique game versions from the promotion keys by splitting off
// the "-recommended"/"-latest" suffix
pub async fn fetch_forge_game_versions(client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
    let promos: ForgePromotions = client.get_json(FORGE_PROMOTIONS_URL).await?;

    let mut game_versions: Vec<String> = promos
        .promos
        .keys()
        .filter_map(|key| key.rsplit_once('-').map(|(version, _)| version.to_string()))
        .collect();
    game_versions.sort();
    game_versions.dedup();
    game_versions.reverse();

    Ok(game_versions
        .into_iter()
        .map(|version| GameVersion {
            id: version,
            stable: true,
        })
        .collect())
}

// forge has used at least three different maven naming conventions over the
// years with no clear cutoff. we just try each one until something works.
pub async fn download_forge_installer(
    client: &HttpClient,
    game_version: &str,
    forge_version: &str,
    dest: &Path,
) -> Result<(), NetError> {
    let mc_no_dots: String = game_version.chars().filter(|c| *c != '.').collect();

    let slugs = [
        format!("{game_version}-{forge_version}"),
        format!("{game_version}-{forge_version}-{game_version}"),
        format!("{game_version}-{forge_version}-mc{mc_no_dots}"),
    ];

    set_action(format!(
        "Downloading Forge {}-{}...",
        game_version, forge_version
    ));

    let mut last_err = None;
    for slug in &slugs {
        let url = format!(
            "{}/{slug}/forge-{slug}-installer.jar",
            FORGE_MAVEN_BASE,
        );
        match download_file(client, &url, dest, |downloaded, total| {
            crate::tui::progress::set_progress(downloaded, total);
        })
        .await
        {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }

    Err(last_err.unwrap_or_else(|| {
        NetError::Parse(format!(
            "No Forge installer found for {game_version}-{forge_version}"
        ))
    }))
}

pub async fn run_forge_installer(
    installer_path: &Path,
    instance_dir: &Path,
    java_path: &str,
) -> Result<(), NetError> {
    use tokio::process::Command;

    set_action("Running Forge installer...");

    let output = match Command::new(java_path)
        .arg("-jar")
        .arg(installer_path)
        .arg("--installClient")
        .current_dir(instance_dir.join(".minecraft"))
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            return Err(NetError::Io(e));
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if stderr.trim().is_empty() {
            format!("exit code {:?}", output.status.code())
        } else {
            stderr.lines().last().unwrap_or("unknown error").to_string()
        };
        return Err(NetError::InstallerFailed(detail));
    }

    Ok(())
}

// old forge installers have an install_profile.json with a "versionInfo" key
// containing everything needed. modern ones don't have this structure.
pub(crate) fn has_legacy_install_profile(installer_path: &Path) -> bool {
    let file = match std::fs::File::open(installer_path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut archive = match zip::ZipArchive::new(file) {
        Ok(a) => a,
        Err(_) => return false,
    };
    let entry = match archive.by_name("install_profile.json") {
        Ok(e) => e,
        Err(_) => return false,
    };
    let value: serde_json::Value = match serde_json::from_reader(entry) {
        Ok(v) => v,
        Err(_) => return false,
    };
    value.get("versionInfo").is_some()
}

// handles old-style forge installation by extracting the universal jar and
// library info directly from the installer, bypassing the GUI-only installer
pub(crate) async fn install_forge_from_profile(
    client: &HttpClient,
    installer_path: &Path,
    meta_dir: &Path,
    profile_filename: &str,
) -> Result<(), NetError> {
    use std::io::Read;

    set_action("Installing legacy Forge from profile...");

    let file = std::fs::File::open(installer_path)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| {
        NetError::Parse(format!("Failed to open installer as ZIP: {e}"))
    })?;

    let profile_data: serde_json::Value = {
        let entry = archive.by_name("install_profile.json").map_err(|e| {
            NetError::Parse(format!("install_profile.json not found in installer: {e}"))
        })?;
        serde_json::from_reader(entry).map_err(|e| {
            NetError::Parse(format!("Failed to parse install_profile.json: {e}"))
        })?
    };

    let version_info = profile_data.get("versionInfo").ok_or_else(|| {
        NetError::Parse("install_profile.json missing versionInfo".into())
    })?;
    let install_info = profile_data.get("install").ok_or_else(|| {
        NetError::Parse("install_profile.json missing install section".into())
    })?;

    let main_class = version_info
        .get("mainClass")
        .and_then(|v| v.as_str())
        .ok_or_else(|| NetError::Parse("missing versionInfo.mainClass".into()))?
        .to_string();

    let libraries = version_info
        .get("libraries")
        .and_then(|v| v.as_array())
        .ok_or_else(|| NetError::Parse("missing versionInfo.libraries".into()))?;

    let file_path = install_info
        .get("filePath")
        .and_then(|v| v.as_str())
        .ok_or_else(|| NetError::Parse("missing install.filePath".into()))?;

    let install_path_coord = install_info
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| NetError::Parse("missing install.path".into()))?;

    // extract the universal jar to the correct maven location
    let universal_maven_path = crate::net::maven_coord_to_path(install_path_coord)
        .ok_or_else(|| {
            NetError::Parse(format!("Invalid maven coord in install.path: {install_path_coord}"))
        })?;

    set_sub_action("Extracting universal JAR...");
    let universal_dest = meta_dir.join("libraries").join(&universal_maven_path);
    if let Some(parent) = universal_dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    {
        let mut entry = archive.by_name(file_path).map_err(|e| {
            NetError::Parse(format!("Universal JAR '{file_path}' not found in installer: {e}"))
        })?;
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        std::fs::write(&universal_dest, &buf)?;
    }

    // download libraries needed by this forge version. libs with a url field
    // are forge-hosted, libs without one are typically from mojang's library
    // server. old forge versions reference libs like launchwrapper that aren't
    // in mojang's modern version metadata, so we fetch those too.
    let libraries_dir = meta_dir.join("libraries");
    for lib in libraries {
        let name = lib.get("name").and_then(|v| v.as_str()).unwrap_or_default();

        let maven_path = match crate::net::maven_coord_to_path(name) {
            Some(p) => p,
            None => {
                return Err(NetError::Parse(format!(
                    "Invalid Maven coordinate: {name}"
                )));
            }
        };

        let dest = libraries_dir.join(&maven_path);
        if dest.exists() {
            continue;
        }

        let base_url = lib
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("https://libraries.minecraft.net/")
            .trim_end_matches('/');
        let download_url = format!("{base_url}/{maven_path}");

        set_sub_action(name);
        download_file(client, &download_url, &dest, |_, _| {}).await?;
    }

    // old forge profiles store game arguments (like --tweakClass) in
    // minecraftArguments. extract non-template args to pass at launch.
    let game_arguments: Vec<String> = version_info
        .get("minecraftArguments")
        .and_then(|v| v.as_str())
        .map(extract_extra_game_args)
        .unwrap_or_default();

    set_action("Saving Forge profile...");
    let lib_entries: Vec<serde_json::Value> = libraries
        .iter()
        .filter_map(|l| {
            l.get("name")
                .and_then(|v| v.as_str())
                .map(|n| serde_json::json!({"name": n}))
        })
        .collect();

    let profile_json = serde_json::json!({
        "mainClass": main_class,
        "libraries": lib_entries,
        "gameArguments": game_arguments,
    });

    crate::instance::loader::save_profile_json(meta_dir, profile_filename, &profile_json)?;

    Ok(())
}

// pulls out game arguments from old forge's minecraftArguments string that
// the launcher doesn't already handle. skips template variables (${...})
// and standard args like --username that we build ourselves.
fn extract_extra_game_args(minecraft_arguments: &str) -> Vec<String> {
    let handled = [
        "--username",
        "--version",
        "--gameDir",
        "--assetsDir",
        "--assetIndex",
        "--uuid",
        "--accessToken",
        "--userProperties",
        "--userType",
    ];

    let tokens: Vec<&str> = minecraft_arguments.split_whitespace().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let token = tokens[i];
        if token.starts_with("--") {
            if handled.contains(&token) {
                // skip the flag and its value
                i += 2;
                continue;
            }
            result.push(token.to_string());
            // if the next token is a value (not a flag or template), include it
            if i + 1 < tokens.len() && !tokens[i + 1].starts_with("--") {
                let val = tokens[i + 1];
                if !val.starts_with("${") {
                    result.push(val.to_string());
                }
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::HttpClient;

    #[tokio::test]
    #[ignore = "hits live Forge API"]
    async fn test_fetch_versions() {
        let client = HttpClient::new();
        match fetch_forge_versions(&client, "1.20.1").await {
            Ok(versions) => {
                assert!(
                    !versions.is_empty(),
                    "Should have Forge versions for 1.20.1"
                );
            }
            Err(e) => panic!("fetch_forge_versions failed: {}", e),
        }
    }

    #[tokio::test]
    #[ignore = "hits live Forge API"]
    async fn test_fetch_game_versions() {
        let client = HttpClient::new();
        match fetch_forge_game_versions(&client).await {
            Ok(versions) => {
                assert!(!versions.is_empty(), "Should have Forge game versions");
                assert!(versions.iter().any(|version| version.id == "1.20.1"));
            }
            Err(e) => panic!("fetch_forge_game_versions failed: {}", e),
        }
    }
}
