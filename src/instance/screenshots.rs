use std::path::{Path, PathBuf};

// width/height are read from the actual image file so the TUI can show
// dimensions. falls back to 1920x1080 if the file is corrupt or unreadable
// because honestly, what else are you gonna pick
#[derive(Debug, Clone)]
pub struct ScreenshotEntry {
    pub name: String,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

pub fn scan_screenshots(instances_dir: &Path, instance_name: &str) -> Vec<ScreenshotEntry> {
    let dir = instances_dir
        .join(instance_name)
        .join(".minecraft")
        .join("screenshots");

    let read_dir = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut entries: Vec<ScreenshotEntry> = read_dir
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.to_string();
            if name.ends_with(".png") || name.ends_with(".jpg") || name.ends_with(".jpeg") {
                let (width, height) = image::image_dimensions(&path).unwrap_or((1920, 1080));
                Some(ScreenshotEntry {
                    name,
                    path,
                    width,
                    height,
                })
            } else {
                None
            }
        })
        .collect();

    // sorted newest-first since minecraft names them with timestamps
    entries.sort_by(|a, b| b.name.cmp(&a.name));
    entries
}
