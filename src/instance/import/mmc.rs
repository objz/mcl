// multimc / prism launcher instance import: mods and configs are bundled
// in the zip. we install the game + loader normally, then extract the
// archive contents over it.

use std::path::Path;

use crate::instance::manager::InstanceManager;
use crate::instance::models::ModLoader;
use crate::tui::progress;

use super::{ImportSummary, PackFormat};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct MmcPack {
    #[serde(default)]
    components: Vec<MmcComponent>,
}

#[derive(Debug, Clone, Deserialize)]
struct MmcComponent {
    uid: String,
    #[serde(default)]
    version: String,
}

impl MmcPack {
    fn game_version(&self) -> Option<String> {
        self.components
            .iter()
            .find(|c| c.uid == "net.minecraft")
            .map(|c| c.version.clone())
    }

    fn loader(&self) -> (Option<ModLoader>, Option<String>) {
        let loaders = [
            ("net.fabricmc.fabric-loader", ModLoader::Fabric),
            ("net.minecraftforge", ModLoader::Forge),
            ("net.neoforged", ModLoader::NeoForge),
            ("org.quiltmc.quilt-loader", ModLoader::Quilt),
        ];

        for (uid, loader) in &loaders {
            if let Some(component) = self.components.iter().find(|c| c.uid == *uid) {
                return (Some(*loader), Some(component.version.clone()));
            }
        }
        (None, None)
    }
}

pub fn build_summary(path: &Path) -> Result<ImportSummary, String> {
    let pack = parse_mmc_pack(path)?;
    tracing::debug!(
        "Parsed MultiMC pack {} with {} component(s)",
        path.display(),
        pack.components.len()
    );

    let game_version = pack
        .game_version()
        .ok_or_else(|| "mmc-pack.json missing net.minecraft component".to_string())?;

    let (loader_opt, loader_version) = pack.loader();
    let loader = loader_opt.unwrap_or(ModLoader::Vanilla);

    let name = instance_name_from_cfg(path).unwrap_or_else(|| "Imported Pack".to_string());

    let (mod_count, override_count) = count_content_files(path)?;
    tracing::trace!(
        "MultiMC summary: name='{}' game_version={} loader={:?} loader_version={:?} mods={} overrides={}",
        name,
        game_version,
        loader,
        loader_version,
        mod_count,
        override_count
    );

    Ok(ImportSummary {
        name,
        pack_version: String::new(),
        game_version,
        loader,
        loader_version,
        mod_count,
        override_count,
        format: PackFormat::Mmc,
        archive_path: path.to_path_buf(),
    })
}

pub async fn execute_import(
    summary: &ImportSummary,
    manager: &InstanceManager,
) -> Result<crate::instance::InstanceConfig, Box<dyn std::error::Error + Send + Sync>> {
    let name = super::unique_instance_name(&summary.name, &manager.instances_dir);
    tracing::info!(
        "Importing MultiMC/Prism pack '{}' as instance '{}'",
        summary.name,
        name
    );

    progress::set_action(format!("Importing '{name}'..."));
    progress::set_sub_action(format!("{} {}", summary.game_version, summary.loader));

    let config = manager
        .create(
            &name,
            &summary.game_version,
            summary.loader,
            summary.loader_version.as_deref(),
        )
        .await
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;

    let minecraft_dir = manager.instances_dir.join(&name).join(".minecraft");
    extract_mmc_archive(&summary.archive_path, &minecraft_dir)?;

    progress::clear();
    tracing::info!(
        "Imported MultiMC/Prism pack '{}' as '{}'",
        summary.name,
        name
    );
    Ok(config)
}

// extracts everything under .minecraft/ from the archive into the instance dir
fn extract_mmc_archive(
    archive_path: &Path,
    minecraft_dir: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::io::Read;

    progress::set_action("Extracting pack contents...".to_string());
    progress::set_sub_action(String::new());

    let file = std::fs::File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    let prefix = find_archive_prefix(&archive);
    let minecraft_prefix = format!("{prefix}.minecraft/");
    tracing::debug!(
        "Extracting MultiMC archive {} with prefix '{}' into {}",
        archive_path.display(),
        prefix,
        minecraft_dir.display()
    );

    let total = archive.len();
    let mut extracted = 0usize;
    let mut dirs = 0usize;
    for i in 0..total {
        let mut entry = archive.by_index(i)?;
        let entry_name = entry.name().to_string();

        let Some(relative) = entry_name.strip_prefix(&minecraft_prefix) else {
            continue;
        };

        if relative.is_empty() || entry_name.ends_with('/') {
            std::fs::create_dir_all(minecraft_dir.join(relative))?;
            dirs += 1;
            continue;
        }

        let dest = minecraft_dir.join(relative);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let filename = relative.rsplit('/').next().unwrap_or(relative);
        progress::set_sub_action(filename.to_string());

        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        tracing::trace!(
            "Extracting MultiMC entry {} to {} ({} bytes)",
            entry_name,
            dest.display(),
            buf.len()
        );
        std::fs::write(&dest, &buf)?;
        extracted += 1;
    }

    tracing::debug!(
        "Extracted {} files and {} directories from MultiMC archive {}",
        extracted,
        dirs,
        archive_path.display()
    );
    Ok(())
}

fn parse_mmc_pack(path: &Path) -> Result<MmcPack, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Cannot open archive: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Invalid ZIP: {e}"))?;

    let entry_name = find_entry(&archive, "mmc-pack.json")
        .ok_or_else(|| "Missing mmc-pack.json in archive".to_string())?;

    let entry = archive
        .by_name(&entry_name)
        .map_err(|e| format!("Failed to read mmc-pack.json: {e}"))?;

    serde_json::from_reader(entry).map_err(|e| format!("Invalid mmc-pack.json: {e}"))
}

fn instance_name_from_cfg(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    let entry_name = find_entry(&archive, "instance.cfg")?;
    tracing::trace!("Reading instance name from {}", entry_name);
    let entry = archive.by_name(&entry_name).ok()?;

    let reader = std::io::BufRead::lines(std::io::BufReader::new(entry));
    for line in reader.map_while(Result::ok) {
        if let Some(value) = line.strip_prefix("name=") {
            let name = value.trim().to_string();
            if !name.is_empty() {
                tracing::debug!("Read MultiMC instance name '{}' from instance.cfg", name);
                return Some(name);
            }
        }
    }
    None
}

// finds the prefix for the archive: empty for flat zips, "DirName/" for nested
fn find_archive_prefix(archive: &zip::ZipArchive<std::fs::File>) -> String {
    for name in archive.file_names() {
        if name.ends_with("mmc-pack.json") {
            return name.strip_suffix("mmc-pack.json").unwrap_or("").to_string();
        }
    }
    String::new()
}

// looks for a file at root or one level deep (some archives nest everything
// under a single top-level directory like "GT New Horizons 2.8.4/")
fn find_entry(archive: &zip::ZipArchive<std::fs::File>, filename: &str) -> Option<String> {
    if archive.file_names().any(|n| n == filename) {
        return Some(filename.to_string());
    }
    for name in archive.file_names() {
        if name.ends_with(&format!("/{filename}")) && name.matches('/').count() == 1 {
            return Some(name.to_string());
        }
    }
    None
}

fn count_content_files(path: &Path) -> Result<(usize, usize), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let prefix = find_archive_prefix(&archive);

    let minecraft_prefix = format!("{prefix}.minecraft/");
    let mods_prefix = format!("{prefix}.minecraft/mods/");

    let mut mod_count = 0;
    let mut override_count = 0;

    for name in archive.file_names() {
        if name.ends_with('/') {
            continue;
        }
        if name.starts_with(&mods_prefix) {
            mod_count += 1;
        } else if name.starts_with(&minecraft_prefix) {
            override_count += 1;
        }
    }

    Ok((mod_count, override_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mmc_pack_json() {
        let json = r#"{
            "formatVersion": 1,
            "components": [
                {
                    "uid": "net.minecraft",
                    "version": "1.7.10",
                    "cachedName": "Minecraft"
                },
                {
                    "uid": "net.minecraftforge",
                    "version": "10.13.4.1614",
                    "cachedName": "Forge"
                }
            ]
        }"#;
        let pack: MmcPack = serde_json::from_str(json).unwrap();
        assert_eq!(pack.game_version(), Some("1.7.10".to_string()));
        let (loader, version) = pack.loader();
        assert_eq!(loader, Some(ModLoader::Forge));
        assert_eq!(version, Some("10.13.4.1614".to_string()));
    }

    #[test]
    fn parse_mmc_pack_vanilla() {
        let json = r#"{
            "formatVersion": 1,
            "components": [
                {"uid": "net.minecraft", "version": "1.21.4"}
            ]
        }"#;
        let pack: MmcPack = serde_json::from_str(json).unwrap();
        assert!(pack.loader().0.is_none());
    }

    // builds an in-memory mmc-style pack zip and verifies that
    // extract_mmc_archive copies only the .minecraft/ subtree into the
    // destination, preserving relative paths and skipping siblings.
    #[test]
    fn extract_mmc_archive_copies_minecraft_subtree() {
        use std::io::Write;

        let tmp = tempfile::tempdir().unwrap();
        let archive_path = tmp.path().join("pack.zip");
        let dest = tmp.path().join("instance/.minecraft");
        std::fs::create_dir_all(&dest).unwrap();

        // Pack/ is the prefix; only .minecraft/ entries should land in dest.
        // mmc-style pack: a root dir "Pack/" wrapping the .minecraft tree
        // plus a sibling mmc-pack.json that should NOT be extracted.
        {
            let file = std::fs::File::create(&archive_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let opts: zip::write::SimpleFileOptions = Default::default();

            zip.start_file("Pack/mmc-pack.json", opts).unwrap();
            zip.write_all(b"{}").unwrap();

            zip.start_file("Pack/.minecraft/options.txt", opts).unwrap();
            zip.write_all(b"lang:en_us").unwrap();

            zip.start_file("Pack/.minecraft/mods/test-mod.jar", opts)
                .unwrap();
            zip.write_all(b"jar-bytes").unwrap();

            zip.finish().unwrap();
        }

        extract_mmc_archive(&archive_path, &dest).expect("extract");

        // .minecraft/ entries must have been copied with their relative paths
        let options = std::fs::read(dest.join("options.txt")).expect("options.txt");
        assert_eq!(options, b"lang:en_us");
        let modjar = std::fs::read(dest.join("mods/test-mod.jar")).expect("mods/test-mod.jar");
        assert_eq!(modjar, b"jar-bytes");

        // and the sibling outside .minecraft/ must not have been copied
        assert!(
            !dest.join("mmc-pack.json").exists(),
            "mmc-pack.json should not land in the instance dir"
        );
    }
}
