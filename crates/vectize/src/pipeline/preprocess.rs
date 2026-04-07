//! Image preprocessing: normalization, optional edge-aware denoising,
//! resampling, and alpha compositing.

use image::{imageops::FilterType, DynamicImage, ImageBuffer, Pixel, Rgba};

use crate::config::TracingConfig;

const EDGE_AWARE_BLUR_DELTA: i16 = 48;

/// Preprocess the image according to the tracing configuration.
///
/// Steps:
/// 1. Convert to RGBA8 (normalizes bit depth)
/// 2. If preprocessing is enabled, optionally apply edge-aware Gaussian blur for denoising
///
/// If `enable_preprocessing` is `false`, only the RGBA8 conversion is performed.
pub fn preprocess(img: &DynamicImage, config: &TracingConfig) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let rgba = img.to_rgba8();

    if !config.enable_preprocessing {
        return rgba;
    }

    if config.enable_denoising {
        gaussian_blur(&rgba, 0.8)
    } else {
        rgba
    }
}

/// Apply a simple 3×3 edge-aware Gaussian blur to reduce noise.
fn gaussian_blur(
    src: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    _sigma: f32,
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let (width, height) = src.dimensions();
    let mut dst = src.clone();

    // 3×3 Gaussian kernel (approximated)
    let kernel: [f32; 9] = [
        1.0 / 16.0,
        2.0 / 16.0,
        1.0 / 16.0,
        2.0 / 16.0,
        4.0 / 16.0,
        2.0 / 16.0,
        1.0 / 16.0,
        2.0 / 16.0,
        1.0 / 16.0,
    ];

    for y in 1..height.saturating_sub(1) {
        for x in 1..width.saturating_sub(1) {
            let mut r = 0.0f32;
            let mut g = 0.0f32;
            let mut b = 0.0f32;
            let mut a = 0.0f32;
            let mut total_weight = 0.0f32;
            let center = src.get_pixel(x, y).0;

            for (ki, (dy, dx)) in [
                (-1i32, -1i32),
                (-1, 0),
                (-1, 1),
                (0, -1),
                (0, 0),
                (0, 1),
                (1, -1),
                (1, 0),
                (1, 1),
            ]
            .iter()
            .enumerate()
            {
                let nx = (x as i32 + dx) as u32;
                let ny = (y as i32 + dy) as u32;
                let px = src.get_pixel(nx, ny);
                if color_delta(center, px.0) > EDGE_AWARE_BLUR_DELTA {
                    continue;
                }

                let channels = px.channels();
                let weight = kernel[ki];
                total_weight += weight;
                r += channels[0] as f32 * weight;
                g += channels[1] as f32 * weight;
                b += channels[2] as f32 * weight;
                a += channels[3] as f32 * weight;
            }

            let norm = total_weight.max(f32::EPSILON);
            dst.put_pixel(
                x,
                y,
                Rgba([
                    (r / norm) as u8,
                    (g / norm) as u8,
                    (b / norm) as u8,
                    (a / norm) as u8,
                ]),
            );
        }
    }

    dst
}

fn color_delta(left: [u8; 4], right: [u8; 4]) -> i16 {
    left.into_iter()
        .zip(right)
        .map(|(lhs, rhs)| (lhs as i16 - rhs as i16).abs())
        .max()
        .unwrap_or_default()
}

/// Resample an image to a denser tracing grid.
pub(crate) fn resample_for_tracing(
    src: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    scale: u32,
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    if scale <= 1 {
        return src.clone();
    }

    image::imageops::resize(
        src,
        src.width().saturating_mul(scale),
        src.height().saturating_mul(scale),
        FilterType::CatmullRom,
    )
}

/// Composite an RGBA image against a solid background color for pixels below the alpha threshold.
///
/// The `bg` tuple specifies the (R, G, B) background color used for transparent
/// pixels and alpha blending.
pub fn composite_against_background(
    src: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    alpha_threshold: u8,
    bg: (u8, u8, u8),
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    let (width, height) = src.dimensions();
    let mut dst = ImageBuffer::new(width, height);
    let (bg_r, bg_g, bg_b) = (bg.0 as f32, bg.1 as f32, bg.2 as f32);

    for (x, y, px) in src.enumerate_pixels() {
        let [r, g, b, a] = px.0;
        if a < alpha_threshold {
            dst.put_pixel(x, y, Rgba([bg.0, bg.1, bg.2, 255]));
        } else {
            // Alpha-blend over the background color
            let alpha = a as f32 / 255.0;
            let nr = ((r as f32 * alpha) + (bg_r * (1.0 - alpha))) as u8;
            let ng = ((g as f32 * alpha) + (bg_g * (1.0 - alpha))) as u8;
            let nb = ((b as f32 * alpha) + (bg_b * (1.0 - alpha))) as u8;
            dst.put_pixel(x, y, Rgba([nr, ng, nb, 255]));
        }
    }

    dst
}

/// Composite an RGBA image against a white background for pixels below the alpha threshold.
pub fn composite_against_white(
    src: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    alpha_threshold: u8,
) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
    composite_against_background(src, alpha_threshold, (255, 255, 255))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn composite_transparent_pixel_becomes_white() {
        let mut img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::new(2, 2);
        img.put_pixel(0, 0, Rgba([100, 100, 100, 0])); // fully transparent
        img.put_pixel(1, 1, Rgba([0, 0, 0, 255])); // fully opaque black

        let result = composite_against_white(&img, 128);
        assert_eq!(result.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(result.get_pixel(1, 1).0, [0, 0, 0, 255]);
    }

    #[test]
    fn edge_aware_blur_preserves_hard_boundaries() {
        let img = ImageBuffer::from_fn(3, 3, |x, _| {
            if x < 2 {
                Rgba([0, 0, 0, 255])
            } else {
                Rgba([255, 255, 255, 255])
            }
        });

        let blurred = gaussian_blur(&img, 0.8);
        let edge_pixel = blurred.get_pixel(1, 1).0;

        assert!(
            edge_pixel[0] < 16,
            "edge-aware blur should not wash hard black/white boundaries into grey: {edge_pixel:?}"
        );
    }

    #[test]
    fn resample_for_tracing_scales_dimensions() {
        let img = ImageBuffer::from_pixel(4, 3, Rgba([10, 20, 30, 255]));
        let scaled = resample_for_tracing(&img, 2);

        assert_eq!(scaled.dimensions(), (8, 6));
        assert_eq!(scaled.get_pixel(4, 2).0, [10, 20, 30, 255]);
    }
}
