pub mod mods;
pub mod resource_packs;
pub mod shaders;
pub mod worlds;

pub use mods::{scan_mods, toggle_entry, ContentEntry};
pub use resource_packs::scan_resource_packs;
pub use shaders::scan_shaders;
pub use worlds::scan_worlds;

use std::io::Read;

/// Determine enabled/disabled state and file stem from a filename.
///
/// Given the file extension (e.g. ".jar", ".zip") and the file name, returns
/// `Some((enabled, file_stem))` or `None` if the file doesn't match.
pub(crate) fn parse_enabled_stem(file_name: &str, ext: &str) -> Option<(bool, String)> {
    let disabled_ext = format!("{ext}.disabled");
    if let Some(stem) = file_name.strip_suffix(&disabled_ext) {
        Some((false, stem.to_string()))
    } else if let Some(stem) = file_name.strip_suffix(ext) {
        Some((true, stem.to_string()))
    } else {
        None
    }
}

/// For directories: check if the name ends in `.disabled`.
pub(crate) fn parse_enabled_stem_dir(file_name: &str) -> (bool, String) {
    if let Some(stem) = file_name.strip_suffix(".disabled") {
        (false, stem.to_string())
    } else {
        (true, file_name.to_string())
    }
}

/// Read icon bytes from a zip archive (pack.png).
pub(crate) fn read_icon_from_zip(
    archive: &mut zip::ZipArchive<std::fs::File>,
) -> Option<Vec<u8>> {
    let mut entry = archive.by_name("pack.png").ok()?;
    let mut buf = Vec::new();
    entry.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// Open a zip file and return the archive handle.
pub(crate) fn open_zip(path: &std::path::Path) -> Option<zip::ZipArchive<std::fs::File>> {
    let file = std::fs::File::open(path).ok()?;
    zip::ZipArchive::new(file).ok()
}
