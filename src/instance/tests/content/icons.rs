// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn quadrant_raster_has_requested_dimensions() {
    let image =
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(2, 2, image::Rgb([12, 34, 56])));
    let rows = make_icon_quadrants_from_image(&image, 7, 3);

    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| row.len() == 7));
    assert!(rows.iter().flatten().all(|cell| cell.symbol == '\u{2588}'));
}

#[test]
fn fallback_icon_is_square_and_contains_a_separate_question_mark_dot() {
    let icon = fallback_icon();

    assert_eq!(icon.len(), 3);
    assert!(icon.iter().all(|row| row.len() == 6));
    assert_eq!(icon[2][2].bg_r, 45);
    assert_eq!(icon[2][2].fg_r, 150);
    assert_eq!(icon[2][3].fg_r, 150);
}
