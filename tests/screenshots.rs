// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// integration tests for the public scan_screenshots API.

use std::path::{Path, PathBuf};

use rmcl::instance::screenshots::scan_screenshots;

fn setup_screenshots_dir(tmp: &Path, instance: &str) -> PathBuf {
    let dir = tmp.join(instance).join("minecraft").join("screenshots");
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
