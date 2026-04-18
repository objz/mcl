// lists available game versions, optionally filtered by mod loader support.
// fetches the mojang manifest and cross-references with loader APIs
// to show only versions that actually work with a given loader.
use std::collections::HashSet;
use std::io;

use clap::ArgMatches;

use crate::cli::instance::parse_loader;
use crate::cli::output::print_table;
use crate::instance::ModLoader;
use crate::net::mojang::{VersionEntry, VersionManifest};
use crate::net::HttpClient;

type CliResult = Result<(), Box<dyn std::error::Error>>;

pub async fn handle_version(matches: &ArgMatches) -> CliResult {
    match matches.subcommand() {
        Some(("list", sub_matches)) => list_versions(sub_matches).await,
        _ => Ok(()),
    }
}

async fn list_versions(matches: &ArgMatches) -> CliResult {
    let client = HttpClient::new();
    let manifest = crate::net::mojang::fetch_version_manifest(&client).await?;
    let snapshots = matches.get_flag("snapshots");
    let loader = matches
        .get_one::<String>("loader")
        .map(|value| parse_loader(value).map_err(io::Error::other))
        .transpose()?;

    // vanilla supports everything by definition, so only fetch loader-specific
    // version lists when the user actually asked for a modded loader
    let supported = match loader {
        Some(ModLoader::Vanilla) | None => None,
        Some(loader) => Some(fetch_supported_versions(&client, loader).await?),
    };

    let rows = filter_manifest_versions(&manifest, supported.as_ref(), snapshots)
        .into_iter()
        .map(|entry| vec![entry.id, entry.version_type])
        .collect::<Vec<_>>();

    print_table(&["Version", "Type"], &rows);
    Ok(())
}

async fn fetch_supported_versions(
    client: &HttpClient,
    loader: ModLoader,
) -> Result<HashSet<String>, Box<dyn std::error::Error>> {
    let versions: HashSet<String> = match loader {
        ModLoader::Vanilla => HashSet::new(),
        ModLoader::Fabric => crate::net::fabric::fetch_fabric_game_versions(client)
            .await?
            .into_iter()
            .map(|version| version.id)
            .collect::<HashSet<_>>(),
        ModLoader::Forge => crate::net::forge::fetch_forge_game_versions(client)
            .await?
            .into_iter()
            .map(|version| version.id)
            .collect::<HashSet<_>>(),
        ModLoader::NeoForge => crate::net::neoforge::fetch_neoforge_game_versions(client)
            .await?
            .into_iter()
            .map(|version| version.id)
            .collect::<HashSet<_>>(),
        ModLoader::Quilt => crate::net::quilt::fetch_quilt_game_versions(client)
            .await?
            .into_iter()
            .map(|version| version.id)
            .collect::<HashSet<_>>(),
    };

    Ok(versions)
}

fn filter_manifest_versions(
    manifest: &VersionManifest,
    supported: Option<&HashSet<String>>,
    snapshots: bool,
) -> Vec<VersionEntry> {
    manifest
        .versions
        .iter()
        .filter(|entry| snapshots || entry.version_type == "release")
        .filter(|entry| {
            supported
                .map(|versions| versions.contains(&entry.id))
                .unwrap_or(true)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::filter_manifest_versions;
    use crate::net::mojang::{LatestVersions, VersionEntry, VersionManifest};
    use std::collections::HashSet;

    fn manifest() -> VersionManifest {
        VersionManifest {
            latest: LatestVersions {
                release: "1.20.1".to_string(),
                snapshot: "24w01a".to_string(),
            },
            versions: vec![
                VersionEntry {
                    id: "1.20.1".to_string(),
                    version_type: "release".to_string(),
                    url: "https://example.com/release".to_string(),
                    sha1: "a".to_string(),
                },
                VersionEntry {
                    id: "24w01a".to_string(),
                    version_type: "snapshot".to_string(),
                    url: "https://example.com/snapshot".to_string(),
                    sha1: "b".to_string(),
                },
            ],
        }
    }

    #[test]
    fn filters_out_snapshots_by_default() {
        let rows = filter_manifest_versions(&manifest(), None, false);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "1.20.1");
    }

    #[test]
    fn intersects_supported_versions() {
        let supported = HashSet::from(["24w01a".to_string()]);
        let rows = filter_manifest_versions(&manifest(), Some(&supported), true);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "24w01a");
    }
}
