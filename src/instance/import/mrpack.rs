// modrinth .mrpack import: parse the manifest, download all the mods,
// and extract config/resource overrides from the zip

use std::path::Path;

use crate::instance::manager::InstanceManager;
use crate::instance::models::ModLoader;
use crate::net::modrinth::MrpackIndex;
use crate::tui::progress;

use super::{ImportSummary, PackFormat};

pub fn build_summary(path: &Path) -> Result<ImportSummary, String> {
    let index = crate::net::modrinth::parse_mrpack(path)?;

    let game_version = crate::net::modrinth::game_version_from_dependencies(&index.dependencies)
        .ok_or_else(|| "Manifest missing minecraft dependency".to_string())?;

    let (loader_opt, loader_version) =
        crate::net::modrinth::loader_from_dependencies(&index.dependencies);
    let loader = loader_opt.unwrap_or(ModLoader::Vanilla);

    let override_count = count_overrides(path).unwrap_or(0);

    Ok(ImportSummary {
        name: index.name.clone(),
        pack_version: index.version_id.clone(),
        game_version,
        loader,
        loader_version,
        mod_count: index.files.len(),
        override_count,
        format: PackFormat::Mrpack,
        archive_path: path.to_path_buf(),
    })
}

// peek into the zip to count files under overrides/ and client-overrides/
fn count_overrides(mrpack_path: &Path) -> Result<usize, String> {
    let file = std::fs::File::open(mrpack_path).map_err(|e| e.to_string())?;
    let archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let count = archive
        .file_names()
        .filter(|name| {
            (name.starts_with("overrides/") || name.starts_with("client-overrides/"))
                && !name.ends_with('/')
        })
        .count();
    Ok(count)
}

pub async fn execute_import(
    summary: &ImportSummary,
    manager: &InstanceManager,
) -> Result<crate::instance::InstanceConfig, Box<dyn std::error::Error + Send + Sync>> {
    let name = super::unique_instance_name(&summary.name, &manager.instances_dir);

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

    let index = crate::net::modrinth::parse_mrpack(&summary.archive_path)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;

    download_mod_files(&index, &minecraft_dir).await?;

    extract_overrides(&summary.archive_path, &minecraft_dir)?;

    progress::clear();
    Ok(config)
}

// downloads all mod files listed in the mrpack index, capped at 10 concurrent
// downloads to avoid getting rate-limited into oblivion
async fn download_mod_files(
    index: &MrpackIndex,
    minecraft_dir: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let client = crate::net::HttpClient::new();
    let total = index.files.len();
    let completed = Arc::new(AtomicUsize::new(0));

    progress::set_action(format!("Downloading mods... 0/{total}"));

    // bounded concurrency via manual JoinSet draining: seed with max_concurrent
    // tasks, then spawn a new one each time one finishes
    let mut tasks = tokio::task::JoinSet::new();
    let max_concurrent = 10;
    let mut file_iter = index.files.iter();

    for _ in 0..max_concurrent {
        if let Some(file) = file_iter.next() {
            let client = client.clone();
            let dest = minecraft_dir.join(&file.path);
            let url = file.downloads.first().cloned().unwrap_or_default();
            let filename = file
                .path
                .rsplit('/')
                .next()
                .unwrap_or(&file.path)
                .to_string();
            let completed = completed.clone();
            tasks.spawn(async move {
                if let Some(parent) = dest.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                progress::set_sub_action(filename);
                crate::net::download_file(&client, &url, &dest, |_, _| {}).await?;
                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                progress::set_action(format!("Downloading mods... {done}/{total}"));
                Ok::<(), crate::net::NetError>(())
            });
        }
    }

    for file in file_iter {
        if let Some(result) = tasks.join_next().await {
            result
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
        }
        let client = client.clone();
        let dest = minecraft_dir.join(&file.path);
        let url = file.downloads.first().cloned().unwrap_or_default();
        let filename = file
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&file.path)
            .to_string();
        let completed = completed.clone();
        tasks.spawn(async move {
            if let Some(parent) = dest.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            progress::set_sub_action(filename);
            crate::net::download_file(&client, &url, &dest, |_, _| {}).await?;
            let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
            progress::set_action(format!("Downloading mods... {done}/{total}"));
            Ok::<(), crate::net::NetError>(())
        });
    }

    while let Some(result) = tasks.join_next().await {
        result
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?
            .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
    }

    Ok(())
}

fn extract_overrides(
    mrpack_path: &Path,
    minecraft_dir: &Path,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use std::io::Read;

    progress::set_action("Extracting overrides...".to_string());
    progress::set_sub_action(String::new());

    let file = std::fs::File::open(mrpack_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let entry_name = entry.name().to_string();

        let relative = entry_name
            .strip_prefix("overrides/")
            .or_else(|| entry_name.strip_prefix("client-overrides/"));

        let Some(relative) = relative else {
            continue;
        };

        if relative.is_empty() || entry_name.ends_with('/') {
            let dir = minecraft_dir.join(relative);
            std::fs::create_dir_all(dir)?;
            continue;
        }

        let dest = minecraft_dir.join(relative);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut buf = Vec::new();
        entry.read_to_end(&mut buf)?;
        std::fs::write(&dest, &buf)?;
    }

    Ok(())
}
