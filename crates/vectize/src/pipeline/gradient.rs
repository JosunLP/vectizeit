//! Approximate smoothly varying region colors with SVG paint servers.

use std::collections::HashMap;

use image::{ImageBuffer, Rgba};

use crate::pipeline::color::{
    lerp_linear_rgb, linear_rgb_to_oklab, linear_rgb_to_srgb, oklab_distance_sq,
    srgb_to_linear_rgb, srgb_to_oklab,
};
use crate::pipeline::segment::{PaletteColor, SegmentationResult};

const MIN_GRADIENT_PIXELS: u32 = 96;
const MIN_GRADIENT_SPAN_PX: f64 = 6.0;
const MIN_GRADIENT_IMPROVEMENT: f64 = 0.20;
const MIN_GRADIENT_STOP_DISTANCE_SQ: f64 = 0.0025;
const LINEAR_QUARTILE_RATIO: f64 = 0.25;
const RADIAL_INNER_RATIO: f64 = 0.35;
const RADIAL_OUTER_RATIO: f64 = 0.70;

pub(crate) type SvgGradientPaintMap = HashMap<PaletteColor, SvgGradientPaint>;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SvgGradientPaint {
    pub id: String,
    pub kind: SvgGradientKind,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SvgGradientKind {
    Linear {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        start: PaletteColor,
        end: PaletteColor,
    },
    Radial {
        cx: f64,
        cy: f64,
        radius: f64,
        inner: PaletteColor,
        outer: PaletteColor,
    },
}

#[derive(Debug, Clone, Copy)]
struct LabelStats {
    count: u32,
    min_x: u32,
    min_y: u32,
    max_x: u32,
    max_y: u32,
    sum_x: f64,
    sum_y: f64,
    sum_r: f64,
    sum_g: f64,
    sum_b: f64,
}

impl Default for LabelStats {
    fn default() -> Self {
        Self {
            count: 0,
            min_x: u32::MAX,
            min_y: u32::MAX,
            max_x: 0,
            max_y: 0,
            sum_x: 0.0,
            sum_y: 0.0,
            sum_r: 0.0,
            sum_g: 0.0,
            sum_b: 0.0,
        }
    }
}

impl LabelStats {
    fn update(&mut self, x: u32, y: u32, pixel: [u8; 3]) {
        let linear = srgb_to_linear_rgb(pixel);

        self.count += 1;
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
        self.sum_x += x as f64 + 0.5;
        self.sum_y += y as f64 + 0.5;
        self.sum_r += linear[0];
        self.sum_g += linear[1];
        self.sum_b += linear[2];
    }

    fn has_pixels(self) -> bool {
        self.count > 0
    }

    fn width(self) -> f64 {
        (self.max_x.saturating_sub(self.min_x) + 1) as f64
    }

    fn height(self) -> f64 {
        (self.max_y.saturating_sub(self.min_y) + 1) as f64
    }

    fn centroid(self) -> (f64, f64) {
        let count = f64::from(self.count.max(1));
        (self.sum_x / count, self.sum_y / count)
    }

    fn mean_linear(self) -> [f64; 3] {
        let count = f64::from(self.count.max(1));
        [self.sum_r / count, self.sum_g / count, self.sum_b / count]
    }

    fn max_radius(self) -> f64 {
        let (cx, cy) = self.centroid();
        [
            (self.min_x as f64, self.min_y as f64),
            (self.max_x as f64 + 1.0, self.min_y as f64),
            (self.min_x as f64, self.max_y as f64 + 1.0),
            (self.max_x as f64 + 1.0, self.max_y as f64 + 1.0),
        ]
        .into_iter()
        .map(|(x, y)| {
            let dx = x - cx;
            let dy = y - cy;
            (dx * dx + dy * dy).sqrt()
        })
        .fold(0.0, f64::max)
        .max(1.0)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct LinearAccumulator {
    sum_r: f64,
    sum_g: f64,
    sum_b: f64,
    count: f64,
}

impl LinearAccumulator {
    fn push(&mut self, pixel: [u8; 3]) {
        let linear = srgb_to_linear_rgb(pixel);
        self.sum_r += linear[0];
        self.sum_g += linear[1];
        self.sum_b += linear[2];
        self.count += 1.0;
    }

    fn average_or(self, fallback: [f64; 3]) -> [f64; 3] {
        if self.count <= f64::EPSILON {
            return fallback;
        }

        [
            self.sum_r / self.count,
            self.sum_g / self.count,
            self.sum_b / self.count,
        ]
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CandidateAccumulators {
    x_start: LinearAccumulator,
    x_end: LinearAccumulator,
    y_start: LinearAccumulator,
    y_end: LinearAccumulator,
    radial_inner: LinearAccumulator,
    radial_outer: LinearAccumulator,
}

#[derive(Debug, Clone, Copy, Default)]
struct CandidateColors {
    x_start: [f64; 3],
    x_end: [f64; 3],
    y_start: [f64; 3],
    y_end: [f64; 3],
    radial_inner: [f64; 3],
    radial_outer: [f64; 3],
}

#[derive(Debug, Clone, Copy, Default)]
struct CandidateErrors {
    baseline: f64,
    linear_x: f64,
    linear_y: f64,
    radial: f64,
}

/// Fit gradients for smooth, sufficiently large labeled regions.
pub(crate) fn approximate_region_gradients(
    img: &ImageBuffer<Rgba<u8>, Vec<u8>>,
    segmentation: &SegmentationResult,
    coordinate_scale: f64,
) -> SvgGradientPaintMap {
    if segmentation.palette.is_empty() || segmentation.labels.is_empty() {
        return HashMap::new();
    }

    let width = segmentation.width as usize;
    let height = segmentation.height as usize;
    let mut stats = vec![LabelStats::default(); segmentation.palette.len()];

    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let label = segmentation.labels[index] as usize;
            let pixel = rgb_at(img, x, y);
            stats[label].update(x as u32, y as u32, pixel);
        }
    }

    let eligible: Vec<bool> = stats
        .iter()
        .map(|stats| {
            stats.has_pixels()
                && stats.count >= MIN_GRADIENT_PIXELS
                && (stats.width() >= MIN_GRADIENT_SPAN_PX || stats.height() >= MIN_GRADIENT_SPAN_PX)
        })
        .collect();

    let mean_linear: Vec<[f64; 3]> = stats.iter().map(|stats| stats.mean_linear()).collect();
    let mean_lab: Vec<_> = mean_linear
        .iter()
        .copied()
        .map(linear_rgb_to_oklab)
        .collect();
    let mut accumulators = vec![CandidateAccumulators::default(); segmentation.palette.len()];
    let mut errors = vec![CandidateErrors::default(); segmentation.palette.len()];

    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let label = segmentation.labels[index] as usize;
            if !eligible[label] {
                continue;
            }

            let pixel = rgb_at(img, x, y);
            let pixel_lab = srgb_to_oklab(pixel);
            let stats = stats[label];
            errors[label].baseline += oklab_distance_sq(pixel_lab, mean_lab[label]);

            let x_pos = x as f64 + 0.5;
            let y_pos = y as f64 + 0.5;
            let x_span = stats.width().max(1.0);
            let y_span = stats.height().max(1.0);
            let left_distance = x_pos - stats.min_x as f64;
            let right_distance = (stats.max_x as f64 + 1.0) - x_pos;
            let top_distance = y_pos - stats.min_y as f64;
            let bottom_distance = (stats.max_y as f64 + 1.0) - y_pos;

            if left_distance <= x_span * LINEAR_QUARTILE_RATIO {
                accumulators[label].x_start.push(pixel);
            }
            if right_distance <= x_span * LINEAR_QUARTILE_RATIO {
                accumulators[label].x_end.push(pixel);
            }
            if top_distance <= y_span * LINEAR_QUARTILE_RATIO {
                accumulators[label].y_start.push(pixel);
            }
            if bottom_distance <= y_span * LINEAR_QUARTILE_RATIO {
                accumulators[label].y_end.push(pixel);
            }

            let (cx, cy) = stats.centroid();
            let dx = x_pos - cx;
            let dy = y_pos - cy;
            let distance = (dx * dx + dy * dy).sqrt();
            let max_radius = stats.max_radius();

            if distance <= max_radius * RADIAL_INNER_RATIO {
                accumulators[label].radial_inner.push(pixel);
            }
            if distance >= max_radius * RADIAL_OUTER_RATIO {
                accumulators[label].radial_outer.push(pixel);
            }
        }
    }

    let candidate_colors: Vec<CandidateColors> = accumulators
        .iter()
        .enumerate()
        .map(|(label, accumulators)| CandidateColors {
            x_start: accumulators.x_start.average_or(mean_linear[label]),
            x_end: accumulators.x_end.average_or(mean_linear[label]),
            y_start: accumulators.y_start.average_or(mean_linear[label]),
            y_end: accumulators.y_end.average_or(mean_linear[label]),
            radial_inner: accumulators.radial_inner.average_or(mean_linear[label]),
            radial_outer: accumulators.radial_outer.average_or(mean_linear[label]),
        })
        .collect();

    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let label = segmentation.labels[index] as usize;
            if !eligible[label] {
                continue;
            }

            let pixel_lab = srgb_to_oklab(rgb_at(img, x, y));
            let stats = stats[label];
            let candidates = candidate_colors[label];
            let x_t = normalized_axis_position(x as f64 + 0.5, stats.min_x, stats.max_x);
            let y_t = normalized_axis_position(y as f64 + 0.5, stats.min_y, stats.max_y);
            let (cx, cy) = stats.centroid();
            let dx = x as f64 + 0.5 - cx;
            let dy = y as f64 + 0.5 - cy;
            let radial_t = ((dx * dx + dy * dy).sqrt() / stats.max_radius()).clamp(0.0, 1.0);

            errors[label].linear_x += oklab_distance_sq(
                pixel_lab,
                linear_rgb_to_oklab(lerp_linear_rgb(candidates.x_start, candidates.x_end, x_t)),
            );
            errors[label].linear_y += oklab_distance_sq(
                pixel_lab,
                linear_rgb_to_oklab(lerp_linear_rgb(candidates.y_start, candidates.y_end, y_t)),
            );
            errors[label].radial += oklab_distance_sq(
                pixel_lab,
                linear_rgb_to_oklab(lerp_linear_rgb(
                    candidates.radial_inner,
                    candidates.radial_outer,
                    radial_t,
                )),
            );
        }
    }

    let mut gradients = HashMap::new();

    for (label, &palette_color) in segmentation.palette.iter().enumerate() {
        if !eligible[label] || errors[label].baseline <= f64::EPSILON {
            continue;
        }

        let stats = stats[label];
        let candidates = candidate_colors[label];
        let best = best_candidate_kind(&stats, &errors[label], &candidates, coordinate_scale);

        let Some(kind) = best else {
            continue;
        };

        let best_error = match kind {
            SvgGradientKind::Linear { x1, y1, x2, y2, .. } if (x1 - x2).abs() > (y1 - y2).abs() => {
                errors[label].linear_x
            }
            SvgGradientKind::Linear { .. } => errors[label].linear_y,
            SvgGradientKind::Radial { .. } => errors[label].radial,
        };

        let improvement = 1.0 - (best_error / errors[label].baseline.max(f64::EPSILON));
        if improvement < MIN_GRADIENT_IMPROVEMENT {
            continue;
        }

        if stop_distance_sq(&kind) < MIN_GRADIENT_STOP_DISTANCE_SQ {
            continue;
        }

        gradients.insert(
            palette_color,
            SvgGradientPaint {
                id: format!(
                    "grad-{:02x}{:02x}{:02x}",
                    palette_color.r, palette_color.g, palette_color.b
                ),
                kind,
            },
        );
    }

    gradients
}

fn best_candidate_kind(
    stats: &LabelStats,
    errors: &CandidateErrors,
    candidates: &CandidateColors,
    coordinate_scale: f64,
) -> Option<SvgGradientKind> {
    let x_mid = (stats.min_y as f64 + stats.max_y as f64 + 1.0) * 0.5 * coordinate_scale;
    let y_mid = (stats.min_x as f64 + stats.max_x as f64 + 1.0) * 0.5 * coordinate_scale;

    let linear_x = SvgGradientKind::Linear {
        x1: stats.min_x as f64 * coordinate_scale,
        y1: x_mid,
        x2: (stats.max_x as f64 + 1.0) * coordinate_scale,
        y2: x_mid,
        start: to_palette_color(candidates.x_start),
        end: to_palette_color(candidates.x_end),
    };
    let linear_y = SvgGradientKind::Linear {
        x1: y_mid,
        y1: stats.min_y as f64 * coordinate_scale,
        x2: y_mid,
        y2: (stats.max_y as f64 + 1.0) * coordinate_scale,
        start: to_palette_color(candidates.y_start),
        end: to_palette_color(candidates.y_end),
    };
    let (cx, cy) = stats.centroid();
    let radial = SvgGradientKind::Radial {
        cx: cx * coordinate_scale,
        cy: cy * coordinate_scale,
        radius: stats.max_radius() * coordinate_scale,
        inner: to_palette_color(candidates.radial_inner),
        outer: to_palette_color(candidates.radial_outer),
    };

    let mut ranked = [
        (errors.linear_x, linear_x),
        (errors.linear_y, linear_y),
        (errors.radial, radial),
    ];
    ranked.sort_by(|left, right| left.0.total_cmp(&right.0));
    ranked.first().map(|(_, kind)| kind.clone())
}

fn normalized_axis_position(position: f64, min: u32, max: u32) -> f64 {
    let span = (max.saturating_sub(min)) as f64;
    if span <= f64::EPSILON {
        0.5
    } else {
        ((position - (min as f64 + 0.5)) / span).clamp(0.0, 1.0)
    }
}

fn stop_distance_sq(kind: &SvgGradientKind) -> f64 {
    match kind {
        SvgGradientKind::Linear { start, end, .. } => oklab_distance_sq(
            srgb_to_oklab([start.r, start.g, start.b]),
            srgb_to_oklab([end.r, end.g, end.b]),
        ),
        SvgGradientKind::Radial { inner, outer, .. } => oklab_distance_sq(
            srgb_to_oklab([inner.r, inner.g, inner.b]),
            srgb_to_oklab([outer.r, outer.g, outer.b]),
        ),
    }
}

fn to_palette_color(color: [f64; 3]) -> PaletteColor {
    let [r, g, b] = linear_rgb_to_srgb(color);
    PaletteColor { r, g, b }
}

fn rgb_at(img: &ImageBuffer<Rgba<u8>, Vec<u8>>, x: usize, y: usize) -> [u8; 3] {
    let [r, g, b, _] = img.get_pixel(x as u32, y as u32).0;
    [r, g, b]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approximate_region_gradients_fits_horizontal_linear_gradient() {
        let img = ImageBuffer::from_fn(32, 8, |x, _| {
            let t = x as f32 / 31.0;
            let r = (255.0 * t).round() as u8;
            let b = (255.0 * (1.0 - t)).round() as u8;
            Rgba([r, 32, b, 255])
        });
        let segmentation = SegmentationResult {
            palette: vec![PaletteColor {
                r: 128,
                g: 32,
                b: 128,
            }],
            labels: vec![0; 32 * 8],
            width: 32,
            height: 8,
        };

        let gradients = approximate_region_gradients(&img, &segmentation, 1.0);
        let gradient = gradients
            .get(&PaletteColor {
                r: 128,
                g: 32,
                b: 128,
            })
            .expect("expected fitted gradient");

        match &gradient.kind {
            SvgGradientKind::Linear {
                x1, x2, start, end, ..
            } => {
                assert!(x2 > x1);
                assert!(start.b > end.b);
                assert!(end.r > start.r);
            }
            SvgGradientKind::Radial { .. } => panic!("expected horizontal linear gradient"),
        }
    }

    #[test]
    fn approximate_region_gradients_skips_flat_regions() {
        let img = ImageBuffer::from_pixel(24, 24, Rgba([40, 80, 120, 255]));
        let segmentation = SegmentationResult {
            palette: vec![PaletteColor {
                r: 40,
                g: 80,
                b: 120,
            }],
            labels: vec![0; 24 * 24],
            width: 24,
            height: 24,
        };

        let gradients = approximate_region_gradients(&img, &segmentation, 1.0);
        assert!(gradients.is_empty());
    }
}
