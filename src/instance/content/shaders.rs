// shader pack scanner backed by the shared pack.mcmeta reader.

use std::path::Path;

use super::entry::ContentEntry;

pub fn scan_one_shader(path: &Path, file_stem: &str, enabled: bool) -> ContentEntry {
    super::packs::scan_one_pack(path, file_stem, enabled)
}

pub fn scan_shaders(instances_dir: &Path, instance_name: &str) -> Vec<ContentEntry> {
    super::packs::scan_packs(instances_dir, instance_name, "shaderpacks")
}
