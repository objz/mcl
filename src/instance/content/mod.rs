// scanning and toggling instance content (mods, resource packs, shaders, worlds).
// minecraft uses a ".disabled" suffix convention for disabled content, so this leans on that heavily.

pub mod entry;
pub mod icons;
pub mod manifest;
pub mod mods;
mod packs;
pub mod provider;
pub mod reconcile;
pub mod resource_packs;
pub mod shaders;
pub mod worlds;

pub(crate) use icons::{
    IconCell, fallback_icon, fallback_icon_large, make_icon_pixels, make_icon_pixels_from_image,
    make_icon_quadrants_from_image,
};
pub use mods::{scan_mods, scan_one_mod};
pub use resource_packs::{scan_one_resource_pack, scan_resource_packs};
pub use shaders::{scan_one_shader, scan_shaders};
pub use worlds::{scan_one_world, scan_worlds};

use std::io::Read;

// figures out if a file is enabled or disabled based on the ".disabled" suffix,
// and strips the extension to get a clean stem name
pub(crate) fn parse_enabled_stem(file_name: &str, ext: &str) -> Option<(bool, String)> {
    let disabled_ext = format!("{ext}.disabled");
    if let Some(stem) = file_name.strip_suffix(&disabled_ext) {
        Some((false, stem.to_string()))
    } else {
        file_name
            .strip_suffix(ext)
            .map(|stem| (true, stem.to_string()))
    }
}

// same idea but for directories, which don't have a file extension to strip
pub(crate) fn parse_enabled_stem_dir(file_name: &str) -> (bool, String) {
    if let Some(stem) = file_name.strip_suffix(".disabled") {
        (false, stem.to_string())
    } else {
        (true, file_name.to_string())
    }
}

pub(crate) fn read_icon_from_zip(archive: &mut zip::ZipArchive<std::fs::File>) -> Option<Vec<u8>> {
    let mut entry = archive.by_name("pack.png").ok()?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).ok()?;
    Some(buf)
}

pub(crate) fn open_zip(path: &std::path::Path) -> Option<zip::ZipArchive<std::fs::File>> {
    let file = std::fs::File::open(path).ok()?;
    zip::ZipArchive::new(file).ok()
}
