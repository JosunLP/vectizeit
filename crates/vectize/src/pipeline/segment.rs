//! Color segmentation using perceptual Oklab palette quantization with
//! deterministic refinement.
//!
//! Reduces the image to a fixed palette and produces a labeled pixel map
//! where each pixel is assigned a color index.

use std::collections::{HashMap, VecDeque};

use image::{ImageBuffer, Rgba};

use crate::config::{QualityPreset, TracingConfig};
use crate::pipeline::color::{
    linear_rgb_to_srgb, oklab_distance_sq, srgb_to_linear_rgb, srgb_to_oklab, OklabColor,
};

const STRONG_ADAPTIVE_FLAT_ART_COLOR_CAP: usize = 12;
const STRONG_ADAPTIVE_FLAT_ART_COVERAGE_NUMERATOR: u32 = 17;
const STRONG_ADAPTIVE_FLAT_ART_COVERAGE_DENOMINATOR: u32 = 18;
const STRONG_ADAPTIVE_FLAT_ART_LABEL_COVERAGE_NUMERATOR: u32 = 17;
const STRONG_ADAPTIVE_FLAT_ART_LABEL_COVERAGE_DENOMINATOR: u32 = 18;
const ADAPTIVE_FLAT_ART_COLOR_CAP: usize = 24;
const ADAPTIVE_FLAT_ART_COVERAGE_NUMERATOR: u32 = 23;
const ADAPTIVE_FLAT_ART_COVERAGE_DENOMINATOR: u32 = 25;
const ADAPTIVE_FLAT_ART_LABEL_COVERAGE_NUMERATOR: u32 = 4;
const ADAPTIVE_FLAT_ART_LABEL_COVERAGE_DENOMINATOR: u32 = 5;
const ADAPTIVE_FLAT_ART_TAIL_SHARE_RATIO: u32 = 50;
const ANTIALIAS_CLEANUP_PASSES: usize = 3;
const ANTIALIAS_MAX_LOCAL_SUPPORT: u8 = 2;
const ANTIALIAS_MIN_DOMINANT_SUPPORT: u8 = 3;
const ANTIALIAS_MIN_COMBINED_SUPPORT: u8 = 5;
const ANTIALIAS_RARITY_RATIO: u32 = 3;
const ANTIALIAS_BRIDGE_DISTANCE_SQ: f64 = 144.0;
const ANTIALIAS_MIN_COLOR_SPAN_SQ: f64 = 1_024.0;
const BRIDGE_LABEL_CLEANUP_PASSES: usize = 2;
const BRIDGE_LABEL_MAX_IMAGE_SHARE_RATIO: u32 = 12;
const BRIDGE_LABEL_RARITY_RATIO: u32 = 3;
const BRIDGE_LABEL_DOMINANT_ADJACENCY_NUMERATOR: u32 = 4;
const BRIDGE_LABEL_DOMINANT_ADJACENCY_DENOMINATOR: u32 = 5;
const PALETTE_REFINEMENT_CYCLES: usize = 3;
const TILED_PALETTE_SAMPLE_LIMIT: usize = 131_072;
const PERCEPTUAL_DISTANCE_EPSILON: f64 = 1e-12;
const TINY_COMPONENT_CLEANUP_PASSES: usize = 2;
const HIGH_DETAIL_TINY_COMPONENT_PALETTE_MIN: usize = 16;
const HIGH_DETAIL_TINY_COMPONENT_MAX_AREA: usize = 32;
const AMBIGUOUS_TINY_COMPONENT_MAX_AREA: usize = 4;

/// An entry in the color palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PaletteAccumulator {
    sum_r: u64,
    sum_g: u64,
    sum_b: u64,
    count: u32,
}

impl PaletteAccumulator {
    fn push(&mut self, pixel: [u8; 3]) {
        self.sum_r += pixel[0] as u64;
        self.sum_g += pixel[1] as u64;
        self.sum_b += pixel[2] as u64;
        self.count += 1;
    }

    fn average_or(self, fallback: [u8; 3]) -> [u8; 3] {
        if self.count == 0 {
            return fallback;
        }

        let count = self.count as u64;
        [
            ((self.sum_r + (count / 2)) / count) as u8,
            ((self.sum_g + (count / 2)) / count) as u8,
            ((self.sum_b + (count / 2)) / count) as u8,
        ]
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct PerceptualAccumulator {
    sum_r: f64,
    sum_g: f64,
    sum_b: f64,
    count: f64,
}

impl PerceptualAccumulator {
    fn push(&mut self, pixel: [u8; 3]) {
        let linear = srgb_to_linear_rgb(pixel);
        self.sum_r += linear[0];
        self.sum_g += linear[1];
        self.sum_b += linear[2];
        self.count += 1.0;
    }

    fn average_or(self, fallback: [u8; 3]) -> [u8; 3] {
        if self.count <= f64::EPSILON {
            return fallback;
        }

        linear_rgb_to_srgb([
            self.sum_r / self.count,
            self.sum_g / self.count,
            self.sum_b / self.count,
        ])
    }
}

trait PixelSource {
    fn len(&self) -> usize;
    fn pixel(&self, index: usize) -> [u8; 3];
}

impl PixelSource for [[u8; 3]] {
    fn len(&self) -> usize {
        <[[u8; 3]]>::len(self)
    }

    fn pixel(&self, index: usize) -> [u8; 3] {
        self[index]
    }
}

impl PixelSource for Vec<[u8; 3]> {
    fn len(&self) -> usize {
        Vec::len(self)
    }

    fn pixel(&self, index: usize) -> [u8; 3] {
        self[index]
    }
}

struct SlicePixelSource<'a> {
    pixels: &'a [[u8; 3]],
}

impl PixelSource for SlicePixelSource<'_> {
    fn len(&self) -> usize {
        self.pixels.len()
    }

    fn pixel(&self, index: usize) -> [u8; 3] {
        self.pixels[index]
    }
}

struct ImagePixelSource<'a> {
    img: &'a ImageBuffer<Rgba<u8>, Vec<u8>>,
    width: usize,
    alpha_threshold: u8,
}

impl<'a> ImagePixelSource<'a> {
    fn new(img: &'a ImageBuffer<Rgba<u8>, Vec<u8>>, alpha_threshold: u8) -> Self {
        Self {
            img,
            width: img.width() as usize,
            alpha_threshold,
        }
    }
}

impl PixelSource for ImagePixelSource<'_> {
    fn len(&self) -> usize {
        self.width * self.img.height() as usize
    }

    fn pixel(&self, index: usize) -> [u8; 3] {
        let x = (index % self.width) as u32;
        let y = (index / self.width) as u32;
        composite_pixel(self.img.get_pixel(x, y).0, self.alpha_threshold)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct HistogramEntry {
    rgb: [u8; 3],
    lab: OklabColor,
    count: u32,
}

struct ComponentBridgeContext<'a> {
    labels: &'a [u8],
    width: usize,
    height: usize,
    primary_label: u8,
    secondary_label: u8,
    palette: &'a [[u8; 3]],
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

/// Quantize the image colors using a deterministic perceptual palette and
/// assign each pixel a palette index.
pub fn quantize(
    img: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    config: &TracingConfig,
) -> SegmentationResult {
    let (width, height) = img.dimensions();

    if let Some(tile_size) = config.tile_size {
        let sample_pixels = collect_palette_samples(img, config.alpha_threshold);
        let max_colors = adaptive_color_budget(&sample_pixels, config.color_count.max(2) as usize);

        let mut result = quantize_tiled_with_budget(
            img,
            &sample_pixels,
            width,
            height,
            max_colors,
            config,
            tile_size,
        );

        if let Some(flat_art_cap) = flat_art_rerun_cap(&result.labels, result.palette.len()) {
            result = quantize_tiled_with_budget(
                img,
                &sample_pixels,
                width,
                height,
                flat_art_cap,
                config,
                tile_size,
            );
        }

        return result;
    }

    let pixels = collect_pixels(img, config.alpha_threshold);
    let max_colors = adaptive_color_budget(&pixels, config.color_count.max(2) as usize);

    let mut result = quantize_with_budget(&pixels, width, height, max_colors, config);

    if let Some(flat_art_cap) = flat_art_rerun_cap(&result.labels, result.palette.len()) {
        result = quantize_with_budget(&pixels, width, height, flat_art_cap, config);
    }

    result
}

fn quantize_with_budget(
    pixels: &[[u8; 3]],
    width: u32,
    height: u32,
    max_colors: usize,
    config: &TracingConfig,
) -> SegmentationResult {
    let palette = deduplicate_palette(refine_palette_perceptual(
        pixels,
        &seed_perceptual_palette(pixels, max_colors),
        PALETTE_REFINEMENT_CYCLES,
    ));
    let mut labels = assign_palette_labels(pixels, &palette);
    let pixel_source = SlicePixelSource { pixels };
    cleanup_antialias_fringes(&pixel_source, &mut labels, &palette, width, height);
    collapse_bridge_palette_labels(&pixel_source, &mut labels, &palette, width, height);
    cleanup_tiny_label_components(
        &pixel_source,
        &mut labels,
        &palette,
        width,
        height,
        tiny_component_cleanup_area_threshold(config, palette.len()),
    );
    let compact_palette = compact_palette(&mut labels, &palette);
    let palette_colors: Vec<PaletteColor> = compact_palette
        .iter()
        .map(|&[r, g, b]| PaletteColor { r, g, b })
        .collect();

    SegmentationResult {
        palette: palette_colors,
        labels,
        width,
        height,
    }
}

fn quantize_tiled_with_budget(
    img: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    sample_pixels: &[[u8; 3]],
    width: u32,
    height: u32,
    max_colors: usize,
    config: &TracingConfig,
    tile_size: u32,
) -> SegmentationResult {
    let palette = deduplicate_palette(refine_palette_perceptual(
        sample_pixels,
        &seed_perceptual_palette(sample_pixels, max_colors),
        PALETTE_REFINEMENT_CYCLES,
    ));
    let mut labels = assign_image_labels_tiled(img, config.alpha_threshold, &palette, tile_size);
    let pixel_source = ImagePixelSource::new(img, config.alpha_threshold);

    cleanup_antialias_fringes(&pixel_source, &mut labels, &palette, width, height);
    collapse_bridge_palette_labels(&pixel_source, &mut labels, &palette, width, height);
    cleanup_tiny_label_components(
        &pixel_source,
        &mut labels,
        &palette,
        width,
        height,
        tiny_component_cleanup_area_threshold(config, palette.len()),
    );

    let compact_palette = compact_palette(&mut labels, &palette);
    let palette_colors: Vec<PaletteColor> = compact_palette
        .iter()
        .map(|&[r, g, b]| PaletteColor { r, g, b })
        .collect();

    SegmentationResult {
        palette: palette_colors,
        labels,
        width,
        height,
    }
}

fn tiny_component_cleanup_area_threshold(config: &TracingConfig, palette_len: usize) -> usize {
    if matches!(config.quality_preset, QualityPreset::High)
        && config.enable_preprocessing
        && config.enable_denoising
        && config.color_count >= 32
        && palette_len >= HIGH_DETAIL_TINY_COMPONENT_PALETTE_MIN
    {
        HIGH_DETAIL_TINY_COMPONENT_MAX_AREA
    } else {
        0
    }
}

fn collect_pixels(img: &ImageBuffer<Rgba<u8>, Vec<u8>>, alpha_threshold: u8) -> Vec<[u8; 3]> {
    img.pixels()
        .map(|px| composite_pixel(px.0, alpha_threshold))
        .collect()
}

fn collect_palette_samples(
    img: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    alpha_threshold: u8,
) -> Vec<[u8; 3]> {
    let (width, height) = img.dimensions();
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let total_pixels = width as usize * height as usize;
    let sample_ratio = total_pixels.div_ceil(TILED_PALETTE_SAMPLE_LIMIT);
    let stride = (sample_ratio as f64).sqrt().ceil().max(1.0) as usize;
    let mut samples =
        Vec::with_capacity((width as usize / stride + 1) * (height as usize / stride + 1));

    for y in (0..height as usize).step_by(stride) {
        for x in (0..width as usize).step_by(stride) {
            samples.push(composite_pixel(
                img.get_pixel(x as u32, y as u32).0,
                alpha_threshold,
            ));
        }
    }

    if samples.is_empty() {
        samples.push(composite_pixel(img.get_pixel(0, 0).0, alpha_threshold));
    }

    samples
}

fn composite_pixel(pixel: [u8; 4], alpha_threshold: u8) -> [u8; 3] {
    let [r, g, b, a] = pixel;
    if a >= alpha_threshold {
        [r, g, b]
    } else {
        [255, 255, 255]
    }
}

fn adaptive_color_budget(pixels: &[[u8; 3]], requested_max_colors: usize) -> usize {
    if requested_max_colors <= STRONG_ADAPTIVE_FLAT_ART_COLOR_CAP || pixels.is_empty() {
        return requested_max_colors;
    }

    let mut histogram: HashMap<[u8; 3], u32> = HashMap::new();
    for &pixel in pixels {
        *histogram.entry(pixel).or_default() += 1;
    }

    let mut counts: Vec<u32> = histogram.into_values().collect();
    counts.sort_unstable_by(|left, right| right.cmp(left));

    let pixel_count = pixels.len() as u32;

    select_flat_art_color_cap(
        &counts,
        requested_max_colors,
        pixel_count,
        &[
            (
                STRONG_ADAPTIVE_FLAT_ART_COLOR_CAP,
                STRONG_ADAPTIVE_FLAT_ART_COVERAGE_NUMERATOR,
                STRONG_ADAPTIVE_FLAT_ART_COVERAGE_DENOMINATOR,
            ),
            (
                ADAPTIVE_FLAT_ART_COLOR_CAP,
                ADAPTIVE_FLAT_ART_COVERAGE_NUMERATOR,
                ADAPTIVE_FLAT_ART_COVERAGE_DENOMINATOR,
            ),
        ],
    )
    .unwrap_or(requested_max_colors)
}

fn flat_art_rerun_cap(labels: &[u8], palette_len: usize) -> Option<usize> {
    if palette_len <= STRONG_ADAPTIVE_FLAT_ART_COLOR_CAP || labels.is_empty() {
        return None;
    }

    let mut counts: Vec<u32> = label_usage(labels, palette_len)
        .into_iter()
        .filter(|&count| count > 0)
        .collect();

    counts.sort_unstable_by(|left, right| right.cmp(left));

    select_flat_art_color_cap(
        &counts,
        palette_len,
        labels.len() as u32,
        &[
            (
                STRONG_ADAPTIVE_FLAT_ART_COLOR_CAP,
                STRONG_ADAPTIVE_FLAT_ART_LABEL_COVERAGE_NUMERATOR,
                STRONG_ADAPTIVE_FLAT_ART_LABEL_COVERAGE_DENOMINATOR,
            ),
            (
                ADAPTIVE_FLAT_ART_COLOR_CAP,
                ADAPTIVE_FLAT_ART_LABEL_COVERAGE_NUMERATOR,
                ADAPTIVE_FLAT_ART_LABEL_COVERAGE_DENOMINATOR,
            ),
        ],
    )
}

fn select_flat_art_color_cap(
    counts: &[u32],
    requested_max_colors: usize,
    total: u32,
    caps: &[(usize, u32, u32)],
) -> Option<usize> {
    caps.iter().find_map(|&(cap, numerator, denominator)| {
        qualifies_for_flat_art_cap(
            counts,
            requested_max_colors,
            total,
            cap,
            numerator,
            denominator,
        )
        .then_some(cap)
    })
}

fn qualifies_for_flat_art_cap(
    counts: &[u32],
    requested_max_colors: usize,
    total: u32,
    cap: usize,
    coverage_numerator: u32,
    coverage_denominator: u32,
) -> bool {
    if requested_max_colors <= cap || counts.is_empty() || total == 0 {
        return false;
    }

    let top_color_coverage: u32 = counts.iter().take(cap).sum();
    let tail_peak = counts.get(cap).copied().unwrap_or_default();

    top_color_coverage * coverage_denominator >= total * coverage_numerator
        && tail_peak * ADAPTIVE_FLAT_ART_TAIL_SHARE_RATIO <= total
}

fn assign_palette_labels(pixels: &[[u8; 3]], palette: &[[u8; 3]]) -> Vec<u8> {
    let palette_labs = build_palette_labs(palette);

    pixels
        .iter()
        .map(|&pixel| nearest_palette_index(pixel, &palette_labs) as u8)
        .collect()
}

fn assign_image_labels_tiled(
    img: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    alpha_threshold: u8,
    palette: &[[u8; 3]],
    tile_size: u32,
) -> Vec<u8> {
    let width = img.width() as usize;
    let height = img.height() as usize;
    let tile_size = tile_size.max(2) as usize;
    let palette_labs = build_palette_labs(palette);
    let mut labels = vec![0u8; width * height];

    for tile_y in (0..height).step_by(tile_size) {
        let end_y = (tile_y + tile_size).min(height);

        for tile_x in (0..width).step_by(tile_size) {
            let end_x = (tile_x + tile_size).min(width);

            for y in tile_y..end_y {
                for x in tile_x..end_x {
                    let index = y * width + x;
                    labels[index] = nearest_palette_index(
                        composite_pixel(img.get_pixel(x as u32, y as u32).0, alpha_threshold),
                        &palette_labs,
                    ) as u8;
                }
            }
        }
    }

    labels
}

fn cleanup_antialias_fringes<P: PixelSource + ?Sized>(
    pixels: &P,
    labels: &mut [u8],
    palette: &[[u8; 3]],
    width: u32,
    height: u32,
) {
    if palette.len() < 3 || labels.len() != pixels.len() || width < 2 || height < 2 {
        return;
    }

    let width = width as usize;
    let height = height as usize;

    for _ in 0..ANTIALIAS_CLEANUP_PASSES {
        let usage = label_usage(labels, palette.len());
        let mut changes = Vec::new();

        for y in 0..height {
            for x in 0..width {
                let index = y * width + x;
                let current = labels[index] as usize;
                let neighbors = neighbor_label_counts(labels, width, height, x, y);
                let current_support = neighbors
                    .iter()
                    .find_map(|&(label, count)| (label as usize == current).then_some(count))
                    .unwrap_or(0);

                if current_support > ANTIALIAS_MAX_LOCAL_SUPPORT {
                    continue;
                }

                let alternatives: Vec<(u8, u8)> = neighbors
                    .into_iter()
                    .filter(|&(label, count)| label as usize != current && count > 0)
                    .collect();

                if alternatives.len() < 2 {
                    continue;
                }

                let (primary_label, primary_support) = alternatives[0];
                let (secondary_label, secondary_support) = alternatives[1];

                if primary_support < ANTIALIAS_MIN_DOMINANT_SUPPORT
                    || primary_support + secondary_support < ANTIALIAS_MIN_COMBINED_SUPPORT
                {
                    continue;
                }

                let primary = primary_label as usize;
                let secondary = secondary_label as usize;

                if usage[current] * ANTIALIAS_RARITY_RATIO > usage[primary].max(usage[secondary]) {
                    continue;
                }

                if !bridges_neighbor_colors(palette[current], palette[primary], palette[secondary])
                {
                    continue;
                }

                let replacement = choose_bridge_replacement(
                    pixels.pixel(index),
                    primary_label,
                    primary_support,
                    secondary_label,
                    secondary_support,
                    palette,
                );

                if replacement != labels[index] {
                    changes.push((index, replacement));
                }
            }
        }

        if changes.is_empty() {
            break;
        }

        for (index, replacement) in changes {
            labels[index] = replacement;
        }
    }
}

fn collapse_bridge_palette_labels<P: PixelSource + ?Sized>(
    pixels: &P,
    labels: &mut [u8],
    palette: &[[u8; 3]],
    width: u32,
    height: u32,
) {
    if palette.len() < 3 || labels.len() != pixels.len() || width < 2 || height < 2 {
        return;
    }

    let width = width as usize;
    let height = height as usize;

    for _ in 0..BRIDGE_LABEL_CLEANUP_PASSES {
        let usage = label_usage(labels, palette.len());
        let adjacency = label_adjacency(labels, width, height, palette.len());
        let total_pixels = labels.len() as u32;
        let mut plans = vec![None; palette.len()];

        for current in 0..palette.len() {
            let current_usage = usage[current];
            if current_usage == 0
                || current_usage * BRIDGE_LABEL_MAX_IMAGE_SHARE_RATIO > total_pixels
            {
                continue;
            }

            let mut neighbors: Vec<(usize, u32)> = adjacency[current]
                .iter()
                .copied()
                .enumerate()
                .filter(|&(label, count)| label != current && count > 0)
                .collect();

            if neighbors.len() < 2 {
                continue;
            }

            neighbors.sort_unstable_by(|left, right| {
                right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0))
            });

            let (primary, primary_adjacency) = neighbors[0];
            let (secondary, secondary_adjacency) = neighbors[1];
            let dominant_adjacency = primary_adjacency + secondary_adjacency;
            let total_adjacency: u32 = neighbors.iter().map(|(_, count)| *count).sum();

            if dominant_adjacency * BRIDGE_LABEL_DOMINANT_ADJACENCY_DENOMINATOR
                < total_adjacency * BRIDGE_LABEL_DOMINANT_ADJACENCY_NUMERATOR
            {
                continue;
            }

            if current_usage * BRIDGE_LABEL_RARITY_RATIO > usage[primary] + usage[secondary] {
                continue;
            }

            if !bridges_neighbor_colors(palette[current], palette[primary], palette[secondary]) {
                continue;
            }

            plans[current] = Some((primary as u8, secondary as u8));
        }

        if plans.iter().all(Option::is_none) {
            break;
        }

        let mut changed = 0usize;

        for y in 0..height {
            for x in 0..width {
                let index = y * width + x;
                let current = labels[index] as usize;
                let Some((primary_label, secondary_label)) = plans[current] else {
                    continue;
                };

                let context = ComponentBridgeContext {
                    labels,
                    width,
                    height,
                    primary_label,
                    secondary_label,
                    palette,
                };

                let replacement =
                    choose_component_bridge_replacement(pixels.pixel(index), x, y, &context);

                if replacement != labels[index] {
                    labels[index] = replacement;
                    changed += 1;
                }
            }
        }

        if changed == 0 {
            break;
        }
    }
}

fn cleanup_tiny_label_components<P: PixelSource + ?Sized>(
    pixels: &P,
    labels: &mut [u8],
    palette: &[[u8; 3]],
    width: u32,
    height: u32,
    max_component_area: usize,
) {
    if max_component_area == 0
        || palette.len() < 2
        || labels.len() != pixels.len()
        || width < 2
        || height < 2
    {
        return;
    }

    let width = width as usize;
    let height = height as usize;

    for _ in 0..TINY_COMPONENT_CLEANUP_PASSES {
        let mut visited = vec![false; labels.len()];
        let mut changes = Vec::new();

        for start in 0..labels.len() {
            if visited[start] {
                continue;
            }

            let current_label = labels[start];
            let mut queue = VecDeque::new();
            queue.push_back(start);
            visited[start] = true;

            let mut component_area = 0usize;
            let mut component_pixels = Vec::with_capacity(max_component_area);
            let mut component_accumulator = PaletteAccumulator::default();
            let mut boundary_counts = vec![0u16; palette.len()];
            let mut track_component = true;

            while let Some(index) = queue.pop_front() {
                component_area += 1;
                if track_component {
                    component_pixels.push(index);
                    component_accumulator.push(pixels.pixel(index));
                    if component_area > max_component_area {
                        track_component = false;
                        component_pixels.clear();
                        boundary_counts.fill(0);
                    }
                }

                let x = index % width;
                let y = index / width;

                if x > 0 {
                    visit_component_neighbor(
                        index - 1,
                        current_label,
                        labels,
                        &mut visited,
                        &mut queue,
                        track_component,
                        &mut boundary_counts,
                    );
                }
                if x + 1 < width {
                    visit_component_neighbor(
                        index + 1,
                        current_label,
                        labels,
                        &mut visited,
                        &mut queue,
                        track_component,
                        &mut boundary_counts,
                    );
                }
                if y > 0 {
                    visit_component_neighbor(
                        index - width,
                        current_label,
                        labels,
                        &mut visited,
                        &mut queue,
                        track_component,
                        &mut boundary_counts,
                    );
                }
                if y + 1 < height {
                    visit_component_neighbor(
                        index + width,
                        current_label,
                        labels,
                        &mut visited,
                        &mut queue,
                        track_component,
                        &mut boundary_counts,
                    );
                }
            }

            if component_area > max_component_area {
                continue;
            }

            let component_mean = component_accumulator.average_or(palette[current_label as usize]);
            let Some(replacement) = choose_tiny_component_replacement(
                component_mean,
                current_label,
                &boundary_counts,
                palette,
            ) else {
                continue;
            };

            let dominant_boundary = boundary_counts[replacement as usize] as usize;
            let boundary_total = boundary_counts
                .iter()
                .map(|&count| count as usize)
                .sum::<usize>();

            if component_area > AMBIGUOUS_TINY_COMPONENT_MAX_AREA
                && dominant_boundary * 2 < boundary_total
            {
                continue;
            }

            changes.extend(
                component_pixels
                    .into_iter()
                    .map(|index| (index, replacement)),
            );
        }

        if changes.is_empty() {
            break;
        }

        for (index, replacement) in changes {
            labels[index] = replacement;
        }
    }
}

fn visit_component_neighbor(
    neighbor_index: usize,
    current_label: u8,
    labels: &[u8],
    visited: &mut [bool],
    queue: &mut VecDeque<usize>,
    track_component: bool,
    boundary_counts: &mut [u16],
) {
    let neighbor_label = labels[neighbor_index];

    if neighbor_label == current_label {
        if !visited[neighbor_index] {
            visited[neighbor_index] = true;
            queue.push_back(neighbor_index);
        }
    } else if track_component {
        boundary_counts[neighbor_label as usize] += 1;
    }
}

fn choose_tiny_component_replacement(
    component_mean: [u8; 3],
    current_label: u8,
    boundary_counts: &[u16],
    palette: &[[u8; 3]],
) -> Option<u8> {
    let mut best_label = None;
    let mut best_support = 0u16;
    let mut best_distance = f64::INFINITY;

    for (label_index, &support) in boundary_counts.iter().enumerate() {
        if label_index == current_label as usize || support == 0 {
            continue;
        }

        let distance = color_distance_sq(component_mean, palette[label_index]);
        if support > best_support || (support == best_support && distance < best_distance) {
            best_label = Some(label_index as u8);
            best_support = support;
            best_distance = distance;
        }
    }

    best_label
}

fn label_usage(labels: &[u8], palette_len: usize) -> Vec<u32> {
    let mut usage = vec![0u32; palette_len];

    for &label in labels {
        usage[label as usize] += 1;
    }

    usage
}

fn neighbor_label_counts(
    labels: &[u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
) -> Vec<(u8, u8)> {
    let x_start = x.saturating_sub(1);
    let y_start = y.saturating_sub(1);
    let x_end = (x + 1).min(width - 1);
    let y_end = (y + 1).min(height - 1);

    let mut counts: Vec<(u8, u8)> = Vec::with_capacity(8);

    for ny in y_start..=y_end {
        for nx in x_start..=x_end {
            if nx == x && ny == y {
                continue;
            }

            let label = labels[ny * width + nx];
            if let Some((_, count)) = counts.iter_mut().find(|(existing, _)| *existing == label) {
                *count += 1;
            } else {
                counts.push((label, 1));
            }
        }
    }

    counts.sort_unstable_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    counts
}

fn label_adjacency(
    labels: &[u8],
    width: usize,
    height: usize,
    palette_len: usize,
) -> Vec<Vec<u32>> {
    let mut adjacency = vec![vec![0u32; palette_len]; palette_len];

    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let current = labels[index];

            if x + 1 < width {
                increment_label_adjacency(&mut adjacency, current, labels[index + 1]);
            }

            if y + 1 < height {
                increment_label_adjacency(&mut adjacency, current, labels[index + width]);
            }
        }
    }

    adjacency
}

fn increment_label_adjacency(adjacency: &mut [Vec<u32>], left: u8, right: u8) {
    if left == right {
        return;
    }

    let left = left as usize;
    let right = right as usize;
    adjacency[left][right] += 1;
    adjacency[right][left] += 1;
}

fn bridges_neighbor_colors(current: [u8; 3], primary: [u8; 3], secondary: [u8; 3]) -> bool {
    let span_sq = euclidean_color_distance_sq(primary, secondary);
    if span_sq < ANTIALIAS_MIN_COLOR_SPAN_SQ {
        return false;
    }

    color_distance_to_segment_sq(current, primary, secondary) <= ANTIALIAS_BRIDGE_DISTANCE_SQ
}

fn choose_bridge_replacement(
    pixel: [u8; 3],
    primary_label: u8,
    primary_support: u8,
    secondary_label: u8,
    secondary_support: u8,
    palette: &[[u8; 3]],
) -> u8 {
    match primary_support.cmp(&secondary_support) {
        std::cmp::Ordering::Greater => primary_label,
        std::cmp::Ordering::Less => secondary_label,
        std::cmp::Ordering::Equal => {
            let primary_distance = color_distance_sq(pixel, palette[primary_label as usize]);
            let secondary_distance = color_distance_sq(pixel, palette[secondary_label as usize]);

            if primary_distance <= secondary_distance {
                primary_label
            } else {
                secondary_label
            }
        }
    }
}

fn choose_component_bridge_replacement(
    pixel: [u8; 3],
    x: usize,
    y: usize,
    context: &ComponentBridgeContext<'_>,
) -> u8 {
    let neighbors = neighbor_label_counts(context.labels, context.width, context.height, x, y);
    let primary_support = neighbors
        .iter()
        .find_map(|&(label, count)| (label == context.primary_label).then_some(count))
        .unwrap_or(0);
    let secondary_support = neighbors
        .iter()
        .find_map(|&(label, count)| (label == context.secondary_label).then_some(count))
        .unwrap_or(0);

    if primary_support == 0 && secondary_support == 0 {
        let primary_distance =
            color_distance_sq(pixel, context.palette[context.primary_label as usize]);
        let secondary_distance =
            color_distance_sq(pixel, context.palette[context.secondary_label as usize]);

        if primary_distance <= secondary_distance {
            context.primary_label
        } else {
            context.secondary_label
        }
    } else {
        choose_bridge_replacement(
            pixel,
            context.primary_label,
            primary_support,
            context.secondary_label,
            secondary_support,
            context.palette,
        )
    }
}

fn compact_palette(labels: &mut [u8], palette: &[[u8; 3]]) -> Vec<[u8; 3]> {
    let mut used = vec![false; palette.len()];
    for &label in labels.iter() {
        used[label as usize] = true;
    }

    let mut remap = vec![u8::MAX; palette.len()];
    let mut compact = Vec::with_capacity(palette.len());

    for (index, &color) in palette.iter().enumerate() {
        if used[index] {
            remap[index] = compact.len() as u8;
            compact.push(color);
        }
    }

    for label in labels.iter_mut() {
        *label = remap[*label as usize];
    }

    compact
}

fn refine_palette_perceptual(
    pixels: &[[u8; 3]],
    seed_palette: &[[u8; 3]],
    cycles: usize,
) -> Vec<[u8; 3]> {
    if pixels.is_empty() || seed_palette.is_empty() || cycles == 0 {
        return seed_palette.to_vec();
    }

    let mut palette = seed_palette.to_vec();
    let mut palette_labs = build_palette_labs(&palette);

    for _ in 0..cycles {
        let mut accumulators = vec![PerceptualAccumulator::default(); palette.len()];

        for &pixel in pixels {
            let index = nearest_palette_index(pixel, &palette_labs);
            accumulators[index].push(pixel);
        }

        let next_palette: Vec<[u8; 3]> = palette
            .iter()
            .copied()
            .zip(accumulators.into_iter())
            .map(|(current, accumulator)| accumulator.average_or(current))
            .collect();

        if next_palette == palette {
            break;
        }

        palette = next_palette;
        palette_labs = build_palette_labs(&palette);
    }

    palette
}

fn build_palette_labs(palette: &[[u8; 3]]) -> Vec<OklabColor> {
    palette.iter().copied().map(srgb_to_oklab).collect()
}

fn nearest_palette_index(pixel: [u8; 3], palette_labs: &[OklabColor]) -> usize {
    let pixel_lab = srgb_to_oklab(pixel);

    palette_labs
        .iter()
        .enumerate()
        .min_by(|(left_index, left), (right_index, right)| {
            oklab_distance_sq(pixel_lab, **left)
                .total_cmp(&oklab_distance_sq(pixel_lab, **right))
                .then_with(|| left_index.cmp(right_index))
        })
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn deduplicate_palette(palette: Vec<[u8; 3]>) -> Vec<[u8; 3]> {
    let mut unique = Vec::with_capacity(palette.len());

    for color in palette {
        if !unique.contains(&color) {
            unique.push(color);
        }
    }

    unique
}

fn color_distance_sq(left: [u8; 3], right: [u8; 3]) -> f64 {
    oklab_distance_sq(srgb_to_oklab(left), srgb_to_oklab(right))
}

fn euclidean_color_distance_sq(left: [u8; 3], right: [u8; 3]) -> f64 {
    let dr = left[0] as f64 - right[0] as f64;
    let dg = left[1] as f64 - right[1] as f64;
    let db = left[2] as f64 - right[2] as f64;

    (dr * dr) + (dg * dg) + (db * db)
}

fn color_distance_to_segment_sq(point: [u8; 3], start: [u8; 3], end: [u8; 3]) -> f64 {
    let start = [start[0] as f64, start[1] as f64, start[2] as f64];
    let end = [end[0] as f64, end[1] as f64, end[2] as f64];
    let point = [point[0] as f64, point[1] as f64, point[2] as f64];
    let segment = [end[0] - start[0], end[1] - start[1], end[2] - start[2]];
    let segment_len_sq =
        (segment[0] * segment[0]) + (segment[1] * segment[1]) + (segment[2] * segment[2]);

    if segment_len_sq <= f64::EPSILON {
        return euclidean_color_distance_sq(
            [point[0] as u8, point[1] as u8, point[2] as u8],
            [start[0] as u8, start[1] as u8, start[2] as u8],
        );
    }

    let t = (((point[0] - start[0]) * segment[0])
        + ((point[1] - start[1]) * segment[1])
        + ((point[2] - start[2]) * segment[2]))
        / segment_len_sq;
    let t = t.clamp(0.0, 1.0);
    let closest = [
        start[0] + (segment[0] * t),
        start[1] + (segment[1] * t),
        start[2] + (segment[2] * t),
    ];

    let dr = point[0] - closest[0];
    let dg = point[1] - closest[1];
    let db = point[2] - closest[2];
    (dr * dr) + (dg * dg) + (db * db)
}

fn seed_perceptual_palette(pixels: &[[u8; 3]], max_colors: usize) -> Vec<[u8; 3]> {
    if pixels.is_empty() {
        return vec![[255, 255, 255]];
    }

    let entries = histogram_entries(pixels);
    if entries.len() <= max_colors {
        return entries.iter().map(|entry| entry.rgb).collect();
    }

    let mut palette = Vec::with_capacity(max_colors);
    let mut selected = vec![false; entries.len()];
    let mut nearest_distances = vec![f64::INFINITY; entries.len()];

    palette.push(entries[0].rgb);
    selected[0] = true;

    for (index, entry) in entries.iter().enumerate() {
        nearest_distances[index] = oklab_distance_sq(entry.lab, entries[0].lab);
    }

    while palette.len() < max_colors {
        let Some((next_index, _)) = entries
            .iter()
            .enumerate()
            .filter(|(index, _)| !selected[*index])
            .max_by(|(left_index, left), (right_index, right)| {
                let left_score = nearest_distances[*left_index] * f64::from(left.count);
                let right_score = nearest_distances[*right_index] * f64::from(right.count);

                left_score
                    .total_cmp(&right_score)
                    .then_with(|| left.count.cmp(&right.count))
                    .then_with(|| right.rgb.cmp(&left.rgb))
            })
        else {
            break;
        };

        if nearest_distances[next_index] <= PERCEPTUAL_DISTANCE_EPSILON {
            break;
        }

        selected[next_index] = true;
        palette.push(entries[next_index].rgb);

        for (index, entry) in entries.iter().enumerate() {
            nearest_distances[index] =
                nearest_distances[index].min(oklab_distance_sq(entry.lab, entries[next_index].lab));
        }
    }

    palette
}

fn histogram_entries(pixels: &[[u8; 3]]) -> Vec<HistogramEntry> {
    let mut histogram: HashMap<[u8; 3], u32> = HashMap::new();
    for &pixel in pixels {
        *histogram.entry(pixel).or_default() += 1;
    }

    let mut entries: Vec<HistogramEntry> = histogram
        .into_iter()
        .map(|(rgb, count)| HistogramEntry {
            rgb,
            lab: srgb_to_oklab(rgb),
            count,
        })
        .collect();

    entries.sort_unstable_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.rgb.cmp(&right.rgb))
    });

    entries
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
        let config = TracingConfig {
            color_count: 8,
            ..TracingConfig::default()
        };
        let result = quantize(&img, &config);
        assert!(!result.palette.is_empty());
        assert_eq!(result.labels.len(), 16);
    }

    #[test]
    fn quantize_preserves_minority_color_cluster_after_refinement() {
        let major = Rgba([24, 0, 0, 255]);
        let accent = Rgba([220, 0, 0, 255]);
        let img =
            ImageBuffer::from_fn(10, 10, |x, y| if y == 0 && x < 10 { accent } else { major });

        let config = TracingConfig {
            color_count: 2,
            ..TracingConfig::default()
        };
        let result = quantize(&img, &config);
        let reds: Vec<u8> = result.palette.iter().map(|color| color.r).collect();

        assert!(reds.iter().any(|&red| red <= 40));
        assert!(reds.iter().any(|&red| red >= 200));
        assert_ne!(result.labels[0], result.labels[10]);
    }

    #[test]
    fn quantize_is_deterministic_for_imbalanced_clusters() {
        let img = ImageBuffer::from_fn(12, 12, |x, y| {
            if y < 2 {
                Rgba([230, 30, 30, 255])
            } else if x < 10 {
                Rgba([20, 30, 30, 255])
            } else {
                Rgba([20, 120, 180, 255])
            }
        });

        let config = TracingConfig {
            color_count: 3,
            ..TracingConfig::default()
        };
        let first = quantize(&img, &config);
        let second = quantize(&img, &config);

        assert_eq!(first.palette, second.palette);
        assert_eq!(first.labels, second.labels);
    }

    #[test]
    fn quantize_tiled_matches_untiled_for_small_images() {
        let img = ImageBuffer::from_fn(12, 12, |x, y| {
            let value = ((x * 21) + (y * 13)) as u8;
            Rgba([value, value.wrapping_mul(3), value.wrapping_mul(5), 255])
        });

        let untiled = quantize(
            &img,
            &TracingConfig {
                color_count: 6,
                ..TracingConfig::default()
            },
        );
        let tiled = quantize(
            &img,
            &TracingConfig {
                color_count: 6,
                tile_size: Some(4),
                ..TracingConfig::default()
            },
        );

        assert_eq!(untiled.palette, tiled.palette);
        assert_eq!(untiled.labels, tiled.labels);
    }

    #[test]
    fn seed_perceptual_palette_preserves_distinct_lightness_clusters() {
        let pixels = vec![[20, 20, 24]; 32]
            .into_iter()
            .chain(vec![[128, 128, 134]; 32])
            .chain(vec![[236, 236, 240]; 32])
            .collect::<Vec<_>>();

        let palette = seed_perceptual_palette(&pixels, 3);
        let mut luminances: Vec<u16> = palette
            .iter()
            .map(|color| u16::from(color[0]) + u16::from(color[1]) + u16::from(color[2]))
            .collect();
        luminances.sort_unstable();

        assert_eq!(palette.len(), 3);
        assert!(luminances[0] < 100);
        assert!(luminances[1] > 300 && luminances[1] < 450);
        assert!(luminances[2] > 650);
    }

    #[test]
    fn adaptive_color_budget_caps_flat_art_histograms() {
        let mut pixels = vec![[255, 255, 255]; 1_000];

        for (index, pixel) in pixels.iter_mut().enumerate() {
            let bucket = (index % 24) as u8;
            *pixel = [bucket, bucket.saturating_mul(2), bucket.saturating_mul(3)];
        }

        assert_eq!(adaptive_color_budget(&pixels, 64), 24);
    }

    #[test]
    fn adaptive_color_budget_prefers_stronger_cap_for_long_tail_flat_art() {
        let mut pixels = Vec::new();

        for bucket in 0..12u8 {
            pixels.extend(std::iter::repeat_n(
                [bucket, bucket.saturating_mul(5), bucket.saturating_mul(9)],
                90,
            ));
        }

        for bucket in 0..40u8 {
            pixels.push([
                200u8.saturating_add(bucket),
                40u8.saturating_add(bucket),
                10u8.saturating_add(bucket),
            ]);
        }

        assert_eq!(adaptive_color_budget(&pixels, 64), 12);
    }

    #[test]
    fn adaptive_color_budget_preserves_rich_gradients() {
        let pixels: Vec<[u8; 3]> = (0..512)
            .map(|index| {
                let value = (index % 256) as u8;
                [value, value.wrapping_mul(3), value.wrapping_mul(5)]
            })
            .collect();

        assert_eq!(adaptive_color_budget(&pixels, 64), 64);
    }

    #[test]
    fn flat_art_rerun_cap_detects_long_tail_palettes() {
        let mut labels = Vec::new();
        labels.extend(std::iter::repeat_n(0u8, 300));
        labels.extend(std::iter::repeat_n(1u8, 280));
        labels.extend(std::iter::repeat_n(2u8, 240));

        for label in 3u8..30 {
            labels.push(label);
        }

        assert_eq!(flat_art_rerun_cap(&labels, 30), Some(12));
    }

    #[test]
    fn flat_art_rerun_cap_uses_standard_cap_when_only_top_twenty_four_dominate() {
        let mut labels = Vec::new();

        for label in 0u8..24 {
            labels.extend(std::iter::repeat_n(label, 30));
        }

        for label in 24u8..44 {
            labels.push(label);
        }

        assert_eq!(flat_art_rerun_cap(&labels, 44), Some(24));
    }

    #[test]
    fn flat_art_rerun_cap_prefers_stronger_cap_for_concentrated_palettes() {
        let mut labels = Vec::new();

        for label in 0u8..12 {
            labels.extend(std::iter::repeat_n(label, 90));
        }

        for label in 12u8..52 {
            labels.push(label);
        }

        assert_eq!(flat_art_rerun_cap(&labels, 52), Some(12));
    }

    #[test]
    fn flat_art_rerun_cap_skips_evenly_distributed_palettes() {
        let mut labels = Vec::new();

        for label in 0u8..32 {
            labels.extend(std::iter::repeat_n(label, 16));
        }

        assert_eq!(flat_art_rerun_cap(&labels, 32), None);
    }

    #[test]
    fn cleanup_antialias_fringes_reassigns_bridge_shade_pixels() {
        let pixels = vec![
            [0, 0, 0],
            [0, 0, 0],
            [255, 128, 0],
            [0, 0, 0],
            [128, 64, 0],
            [255, 128, 0],
            [0, 0, 0],
            [0, 0, 0],
            [255, 128, 0],
        ];
        let palette = vec![[0, 0, 0], [128, 64, 0], [255, 128, 0]];
        let mut labels = vec![0, 0, 2, 0, 1, 2, 0, 0, 2];

        cleanup_antialias_fringes(&pixels, &mut labels, &palette, 3, 3);
        let compact = compact_palette(&mut labels, &palette);

        assert_eq!(compact.len(), 2);
        assert!(labels.iter().all(|&label| label != 2));
    }

    #[test]
    fn cleanup_antialias_fringes_keeps_non_bridge_accent_pixels() {
        let pixels = vec![
            [0, 0, 0],
            [0, 0, 0],
            [255, 128, 0],
            [0, 0, 0],
            [0, 64, 255],
            [255, 128, 0],
            [0, 0, 0],
            [0, 0, 0],
            [255, 128, 0],
        ];
        let palette = vec![[0, 0, 0], [0, 64, 255], [255, 128, 0]];
        let mut labels = vec![0, 0, 2, 0, 1, 2, 0, 0, 2];

        cleanup_antialias_fringes(&pixels, &mut labels, &palette, 3, 3);
        let compact = compact_palette(&mut labels, &palette);

        assert_eq!(compact.len(), 3);
        assert_eq!(labels[4], 1);
    }

    #[test]
    fn tiny_component_cleanup_threshold_only_applies_to_rich_high_detail_configs() {
        let high = QualityPreset::High.to_config();
        let balanced = QualityPreset::Balanced.to_config();

        assert_eq!(tiny_component_cleanup_area_threshold(&high, 24), 32);
        assert_eq!(tiny_component_cleanup_area_threshold(&high, 8), 0);
        assert_eq!(tiny_component_cleanup_area_threshold(&balanced, 24), 0);
    }

    #[test]
    fn cleanup_tiny_label_components_reassigns_small_islands() {
        let mut labels = vec![
            0, 0, 0, 0, 0, //
            0, 1, 1, 0, 0, //
            0, 1, 1, 0, 0, //
            0, 0, 0, 0, 0, //
            0, 0, 0, 0, 0,
        ];
        let pixels = labels
            .iter()
            .map(|&label| if label == 0 { [8, 8, 16] } else { [12, 12, 20] })
            .collect::<Vec<_>>();
        let palette = vec![[8, 8, 16], [12, 12, 20], [200, 80, 120]];

        cleanup_tiny_label_components(&pixels, &mut labels, &palette, 5, 5, 4);

        assert!(labels.iter().all(|&label| label == 0));
    }

    #[test]
    fn cleanup_tiny_label_components_preserves_long_features_above_limit() {
        let mut labels = vec![
            0, 0, 1, 0, 0, //
            0, 0, 1, 0, 0, //
            0, 0, 1, 0, 0, //
            0, 0, 1, 0, 0, //
            0, 0, 1, 0, 0,
        ];
        let pixels = labels
            .iter()
            .map(|&label| if label == 0 { [8, 8, 16] } else { [16, 16, 28] })
            .collect::<Vec<_>>();
        let palette = vec![[8, 8, 16], [16, 16, 28]];

        cleanup_tiny_label_components(&pixels, &mut labels, &palette, 5, 5, 4);

        assert_eq!(labels.iter().filter(|&&label| label == 1).count(), 5);
    }
}
