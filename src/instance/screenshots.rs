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

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_screenshots_dir(tmp: &Path, instance: &str) -> PathBuf {
        let dir = tmp.join(instance).join(".minecraft").join("screenshots");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn tiny_png() -> Vec<u8> {
        let img = image::RgbImage::from_pixel(1, 1, image::Rgb([255, 255, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    #[test]
    fn scan_screenshots_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        setup_screenshots_dir(tmp.path(), "inst");
        let screenshots = scan_screenshots(tmp.path(), "inst");
        assert!(screenshots.is_empty());
    }

    #[test]
    fn scan_screenshots_missing_dir_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let screenshots = scan_screenshots(tmp.path(), "ghost");
        assert!(screenshots.is_empty());
    }

    #[test]
    fn scan_screenshots_finds_images() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_screenshots_dir(tmp.path(), "inst");
        std::fs::write(dir.join("2024-01-01.png"), tiny_png()).unwrap();
        std::fs::write(dir.join("2024-01-02.png"), tiny_png()).unwrap();
        let screenshots = scan_screenshots(tmp.path(), "inst");
        assert_eq!(screenshots.len(), 2);
    }

    #[test]
    fn scan_screenshots_ignores_non_images() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_screenshots_dir(tmp.path(), "inst");
        std::fs::write(dir.join("pic.png"), tiny_png()).unwrap();
        std::fs::write(dir.join("notes.txt"), "not an image").unwrap();
        let screenshots = scan_screenshots(tmp.path(), "inst");
        assert_eq!(screenshots.len(), 1);
    }

    #[test]
    fn scan_screenshots_sorted_newest_first() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_screenshots_dir(tmp.path(), "inst");
        std::fs::write(dir.join("aaa.png"), tiny_png()).unwrap();
        std::fs::write(dir.join("zzz.png"), tiny_png()).unwrap();
        let screenshots = scan_screenshots(tmp.path(), "inst");
        assert_eq!(screenshots[0].name, "zzz.png");
        assert_eq!(screenshots[1].name, "aaa.png");
    }

    #[test]
    fn scan_screenshots_reads_dimensions() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_screenshots_dir(tmp.path(), "inst");
        std::fs::write(dir.join("shot.png"), tiny_png()).unwrap();
        let screenshots = scan_screenshots(tmp.path(), "inst");
        assert_eq!(screenshots[0].width, 1);
        assert_eq!(screenshots[0].height, 1);
    }

    #[test]
    fn scan_screenshots_bad_image_uses_default_dimensions() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = setup_screenshots_dir(tmp.path(), "inst");
        std::fs::write(dir.join("corrupt.png"), b"not a real png").unwrap();
        let screenshots = scan_screenshots(tmp.path(), "inst");
        assert_eq!(screenshots.len(), 1);
        assert_eq!(screenshots[0].width, 1920);
        assert_eq!(screenshots[0].height, 1080);
    }
}
