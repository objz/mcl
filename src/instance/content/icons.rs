// SPDX-FileCopyrightText: 2026 Constantin Bauer
// SPDX-License-Identifier: GPL-3.0-only

// terminal icon rasterization shared by every content type and markdown.

// a single "pixel" in a terminal icon.
#[derive(Debug, Clone, Copy)]
pub struct IconCell {
    pub symbol: char,
    pub bg_r: u8,
    pub bg_g: u8,
    pub bg_b: u8,
    pub fg_r: u8,
    pub fg_g: u8,
    pub fg_b: u8,
}

pub(crate) fn make_icon_pixels(
    bytes: &[u8],
    width: u16,
    height: u16,
) -> Option<Vec<Vec<IconCell>>> {
    let image = image::load_from_memory(bytes).ok()?;
    Some(make_icon_pixels_from_image(&image, width, height))
}

pub(crate) fn make_icon_pixels_from_image(
    image: &image::DynamicImage,
    width: u16,
    height: u16,
) -> Vec<Vec<IconCell>> {
    let resized = image.resize_exact(
        u32::from(width),
        u32::from(height) * 2,
        image::imageops::FilterType::Nearest,
    );
    let rgb = resized.to_rgb8();

    let mut rows = Vec::new();
    for row in 0..height {
        let mut columns = Vec::new();
        for column in 0..width {
            let top_y = u32::from(row) * 2;
            let bottom_y = (top_y + 1).min(rgb.height().saturating_sub(1));
            let [tr, tg, tb] = rgb.get_pixel(u32::from(column), top_y).0;
            let [br, bg, bb] = rgb.get_pixel(u32::from(column), bottom_y).0;
            columns.push(IconCell {
                symbol: '\u{2584}',
                bg_r: br,
                bg_g: bg,
                bg_b: bb,
                fg_r: tr,
                fg_g: tg,
                fg_b: tb,
            });
        }
        rows.push(columns);
    }
    rows
}

pub(crate) fn make_icon_quadrants_from_image(
    image: &image::DynamicImage,
    width: u16,
    height: u16,
) -> Vec<Vec<IconCell>> {
    let resized = image
        .resize_exact(
            u32::from(width) * 2,
            u32::from(height) * 2,
            image::imageops::FilterType::Lanczos3,
        )
        .to_rgb8();

    (0..height)
        .map(|row| {
            (0..width)
                .map(|column| {
                    let x = u32::from(column) * 2;
                    let y = u32::from(row) * 2;
                    quadrant_cell([
                        resized.get_pixel(x, y).0,
                        resized.get_pixel(x + 1, y).0,
                        resized.get_pixel(x, y + 1).0,
                        resized.get_pixel(x + 1, y + 1).0,
                    ])
                })
                .collect()
        })
        .collect()
}

fn quadrant_cell(pixels: [[u8; 3]; 4]) -> IconCell {
    let mut pair = (0, 0);
    let mut max_distance = 0;
    for left in 0..pixels.len() {
        for right in (left + 1)..pixels.len() {
            let distance = color_distance(pixels[left], pixels[right]);
            if distance > max_distance {
                max_distance = distance;
                pair = (left, right);
            }
        }
    }

    let bg = pixels[pair.0];
    let fg = pixels[pair.1];
    let mask = pixels
        .iter()
        .enumerate()
        .fold(0_u8, |mask, (index, pixel)| {
            if color_distance(*pixel, fg) <= color_distance(*pixel, bg) {
                mask | (1 << index)
            } else {
                mask
            }
        });
    let symbol = match mask {
        0 => ' ',
        1 => '\u{2598}',
        2 => '\u{259d}',
        3 => '\u{2580}',
        4 => '\u{2596}',
        5 => '\u{258c}',
        6 => '\u{259e}',
        7 => '\u{259b}',
        8 => '\u{2597}',
        9 => '\u{259a}',
        10 => '\u{2590}',
        11 => '\u{259c}',
        12 => '\u{2584}',
        13 => '\u{2599}',
        14 => '\u{259f}',
        _ => '\u{2588}',
    };

    IconCell {
        symbol,
        bg_r: bg[0],
        bg_g: bg[1],
        bg_b: bg[2],
        fg_r: fg[0],
        fg_g: fg[1],
        fg_b: fg[2],
    }
}

fn color_distance(left: [u8; 3], right: [u8; 3]) -> u32 {
    left.into_iter()
        .zip(right)
        .map(|(left, right)| {
            let delta = i32::from(left) - i32::from(right);
            (delta * delta) as u32
        })
        .sum()
}

// 6 columns by 3 terminal rows is physically square with the usual 1:2 cell ratio.
pub(crate) fn fallback_icon() -> Vec<Vec<IconCell>> {
    const MASK: [[bool; 6]; 6] = [
        [false, true, true, true, true, false],
        [true, false, false, false, false, true],
        [false, false, false, true, true, false],
        [false, false, true, true, false, false],
        [false, false, false, false, false, false],
        [false, false, true, true, false, false],
    ];
    MASK.chunks_exact(2)
        .map(|pair| {
            pair[0]
                .iter()
                .zip(pair[1])
                .map(|(&top, bottom)| fallback_cell(top, bottom))
                .collect()
        })
        .collect()
}

fn fallback_cell(top: bool, bottom: bool) -> IconCell {
    let background = if top { 150 } else { 45 };
    let foreground = if bottom { 150 } else { 45 };
    IconCell {
        symbol: '\u{2584}',
        bg_r: background,
        bg_g: background,
        bg_b: background,
        fg_r: foreground,
        fg_g: foreground,
        fg_b: foreground,
    }
}

pub(crate) fn fallback_icon_large() -> Vec<Vec<IconCell>> {
    let background = IconCell {
        symbol: '\u{2584}',
        bg_r: 50,
        bg_g: 50,
        bg_b: 50,
        fg_r: 50,
        fg_g: 50,
        fg_b: 50,
    };
    let bottom = IconCell {
        symbol: '\u{2584}',
        bg_r: 50,
        bg_g: 50,
        bg_b: 50,
        fg_r: 130,
        fg_g: 130,
        fg_b: 130,
    };
    let top = IconCell {
        symbol: '\u{2584}',
        bg_r: 130,
        bg_g: 130,
        bg_b: 130,
        fg_r: 50,
        fg_g: 50,
        fg_b: 50,
    };
    vec![
        vec![
            background, background, bottom, bottom, bottom, bottom, bottom, bottom, bottom, bottom,
            background, background,
        ],
        vec![
            background, background, bottom, bottom, bottom, bottom, bottom, bottom, bottom, bottom,
            background, background,
        ],
        vec![
            background, background, background, background, background, background, top, top, top,
            top, background, background,
        ],
        vec![
            background, background, background, background, background, background, top, top, top,
            top, background, background,
        ],
        vec![
            background, background, background, background, top, top, top, top, background,
            background, background, background,
        ],
        vec![
            background, background, background, background, top, top, top, top, background,
            background, background, background,
        ],
    ]
}

#[cfg(test)]
#[path = "../tests/content/icons.rs"]
mod tests;
