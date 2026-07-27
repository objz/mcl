// resource pack scanner backed by the shared pack.mcmeta reader.

use std::path::Path;

use super::ContentEntry;

pub fn scan_one_resource_pack(path: &Path, file_stem: &str, enabled: bool) -> ContentEntry {
    super::packs::scan_one_pack(path, file_stem, enabled)
}

pub fn scan_resource_packs(instances_dir: &Path, instance_name: &str) -> Vec<ContentEntry> {
    super::packs::scan_packs(instances_dir, instance_name, "resourcepacks")
}

#[cfg(test)]
#[path = "../tests/content/resource_packs.rs"]
mod tests;
