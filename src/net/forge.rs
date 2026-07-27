// forge mod loader: version discovery via promotions API, download and
// installation. modern forge runs a java installer, old forge (pre-1.13ish)
// doesn't support headless install so we extract directly from the jar.

use std::collections::HashMap;
use std::path::Path;

use serde::Deserialize;

use crate::feedback::progress::set_action;
use crate::instance::loader::GameVersion;
use crate::net::{HttpClient, NetError, download_file};

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
    fetch_forge_versions_from(client, FORGE_PROMOTIONS_URL, game_version).await
}

// same as fetch_forge_versions but lets tests point at a wiremock server.
pub async fn fetch_forge_versions_from(
    client: &HttpClient,
    promotions_url: &str,
    game_version: &str,
) -> Result<Vec<String>, NetError> {
    let promotions: ForgePromotions = client.get_json(promotions_url).await?;

    let prefix = format!("{}-", game_version);
    let mut versions: Vec<String> = promotions
        .promos
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .map(|(_, value)| value.clone())
        .collect();

    versions.sort();
    versions.dedup();
    tracing::debug!(
        "Resolved {} Forge version(s) for Minecraft {} from promotions",
        versions.len(),
        game_version
    );
    Ok(versions)
}

// extracts unique game versions from the promotion keys by splitting off
// the "-recommended"/"-latest" suffix
pub async fn fetch_forge_game_versions(client: &HttpClient) -> Result<Vec<GameVersion>, NetError> {
    fetch_forge_game_versions_from(client, FORGE_PROMOTIONS_URL).await
}

pub async fn fetch_forge_game_versions_from(
    client: &HttpClient,
    promotions_url: &str,
) -> Result<Vec<GameVersion>, NetError> {
    let promos: ForgePromotions = client.get_json(promotions_url).await?;

    let mut game_versions: Vec<String> = promos
        .promos
        .keys()
        .filter_map(|key| key.rsplit_once('-').map(|(version, _)| version.to_string()))
        .collect();
    game_versions.sort();
    game_versions.dedup();
    game_versions.reverse();
    tracing::debug!(
        "Resolved {} Forge game version(s) from promotions",
        game_versions.len()
    );

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
        let url = format!("{}/{slug}/forge-{slug}-installer.jar", FORGE_MAVEN_BASE,);
        tracing::debug!("Trying Forge installer slug '{}'", slug);
        match download_file(client, &url, dest, |downloaded, total| {
            crate::feedback::progress::set_progress(downloaded, total);
        })
        .await
        {
            Ok(()) => {
                tracing::debug!("Downloaded Forge installer using slug '{}'", slug);
                return Ok(());
            }
            Err(e) => {
                tracing::debug!("Forge installer slug '{}' failed: {}", slug, e);
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        NetError::Parse(format!(
            "No Forge installer found for {game_version}-{forge_version}"
        ))
    }))
}
