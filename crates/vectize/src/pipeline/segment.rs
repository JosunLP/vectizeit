//! Color segmentation using median-cut color quantization.
//!
//! Reduces the image to a fixed palette and produces a labeled pixel map
//! where each pixel is assigned a color index.

use image::{ImageBuffer, Rgba};

/// An entry in the color palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl PaletteColor {
    /// Returns the CSS hex color string `#rrggbb`.
    pub fn to_hex(&self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }

    /// Squared Euclidean distance in RGB space.
    fn distance_sq(self, other: PaletteColor) -> u32 {
        let dr = self.r as i32 - other.r as i32;
        let dg = self.g as i32 - other.g as i32;
        let db = self.b as i32 - other.b as i32;
        (dr * dr + dg * dg + db * db) as u32
    }
}

/// Result of color segmentation.
pub struct SegmentationResult {
    /// Color palette (up to `color_count` entries).
    pub palette: Vec<PaletteColor>,
    /// Label map: same dimensions as the source image; each value is a palette index.
    pub labels: Vec<u8>,
    /// Image width.
    pub width: u32,
    /// Image height.
    pub height: u32,
}

/// Quantize the image colors using a median-cut approach and assign each pixel
/// a palette index.
pub fn quantize(
    img: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    color_count: u8,
    alpha_threshold: u8,
) -> SegmentationResult {
    let (width, height) = img.dimensions();
    let max_colors = color_count.max(2) as usize;

    // Collect opaque pixels as RGB triples
    let pixels: Vec<[u8; 3]> = img
        .pixels()
        .map(|px| {
            let [r, g, b, a] = px.0;
            if a >= alpha_threshold {
                [r, g, b]
            } else {
                [255u8, 255u8, 255u8] // transparent → white
            }
        })
        .collect();

    let palette = median_cut(&pixels, max_colors);
    let palette_colors: Vec<PaletteColor> = palette
        .iter()
        .map(|&[r, g, b]| PaletteColor { r, g, b })
        .collect();

    // Assign each pixel to the nearest palette color
    let labels: Vec<u8> = pixels
        .iter()
        .map(|&[r, g, b]| {
            let px = PaletteColor { r, g, b };
            palette_colors
                .iter()
                .enumerate()
                .min_by_key(|(_, &c)| px.distance_sq(c))
                .map(|(i, _)| i as u8)
                .unwrap_or(0)
        })
        .collect();

    SegmentationResult {
        palette: palette_colors,
        labels,
        width,
        height,
    }
}

/// Recursive median-cut color quantization.
/// Returns at most `max_colors` representative colors.
fn median_cut(pixels: &[[u8; 3]], max_colors: usize) -> Vec<[u8; 3]> {
    if pixels.is_empty() {
        return vec![[255, 255, 255]];
    }
    let mut buckets: Vec<Vec<[u8; 3]>> = vec![pixels.to_vec()];

    while buckets.len() < max_colors {
        // Find the bucket with the largest range in any channel
        let Some(split_idx) = buckets
            .iter()
            .enumerate()
            .max_by_key(|(_, b)| channel_range(b))
            .map(|(i, _)| i)
        else {
            break;
        };

        let bucket = buckets.remove(split_idx);
        if bucket.len() < 2 {
            buckets.push(bucket);
            break;
        }

        let ch = widest_channel(&bucket);
        let mut sorted = bucket;
        sorted.sort_unstable_by_key(|px| px[ch]);
        let mid = sorted.len() / 2;
        let (left, right) = sorted.split_at(mid);
        buckets.push(left.to_vec());
        buckets.push(right.to_vec());
    }

    buckets.iter().map(|b| average_color(b)).collect()
}

fn channel_range(pixels: &[[u8; 3]]) -> u32 {
    let mut min = [255u8; 3];
    let mut max = [0u8; 3];
    for px in pixels {
        for c in 0..3 {
            min[c] = min[c].min(px[c]);
            max[c] = max[c].max(px[c]);
        }
    }
    (0..3).map(|c| (max[c] - min[c]) as u32).sum()
}

fn widest_channel(pixels: &[[u8; 3]]) -> usize {
    let mut min = [255u8; 3];
    let mut max = [0u8; 3];
    for px in pixels {
        for c in 0..3 {
            min[c] = min[c].min(px[c]);
            max[c] = max[c].max(px[c]);
        }
    }
    (0..3).max_by_key(|&c| max[c] - min[c]).unwrap_or(0)
}

fn average_color(pixels: &[[u8; 3]]) -> [u8; 3] {
    if pixels.is_empty() {
        return [255, 255, 255];
    }
    let n = pixels.len() as u32;
    let (sr, sg, sb) = pixels.iter().fold((0u32, 0u32, 0u32), |(sr, sg, sb), px| {
        (sr + px[0] as u32, sg + px[1] as u32, sb + px[2] as u32)
    });
    [(sr / n) as u8, (sg / n) as u8, (sb / n) as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_color_hex() {
        let c = PaletteColor {
            r: 255,
            g: 0,
            b: 128,
        };
        assert_eq!(c.to_hex(), "#ff0080");
    }

    #[test]
    fn quantize_single_color() {
        let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(4, 4);
        for px in img.pixels_mut() {
            *px = Rgba([200, 100, 50, 255]);
        }
        let result = quantize(&img, 8, 128);
        assert!(!result.palette.is_empty());
        assert_eq!(result.labels.len(), 16);
    }
}
