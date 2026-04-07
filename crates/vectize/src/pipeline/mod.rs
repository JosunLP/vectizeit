//! The multi-stage vectorization pipeline.
//!
//! Orchestrates all processing stages from raw image input to SVG output.
//!
//! ## Pipeline Stages
//!
//! 1. **Preprocessing** – normalize to RGBA8, optionally denoise
//! 2. **Alpha compositing** – composite transparent pixels against white
//! 3. **Color quantization** – median-cut palette reduction with deterministic refinement,
//!    anti-aliased fringe cleanup, and adaptive flat-art palette capping
//! 4. **Contour extraction** – deterministic grid-edge tracing with hole preservation
//! 5. **Despeckle** – remove tiny contours below the threshold
//! 6. **Region assembly** – build color regions from contour data
//! 7. **SVG generation** – simplification, Laplacian smoothing, curve fitting, and SVG emission

pub mod contour;
pub mod curves;
pub mod loader;
pub mod preprocess;
pub mod segment;
pub mod simplify;
pub mod smooth;
pub mod svg;

use image::DynamicImage;
use log::debug;

use crate::config::{QualityPreset, TracingConfig};
use crate::error::Result;
use crate::result::{TraceDebugInfo, TraceStageMetrics, TracedRegionSummary, TracingResult};

use self::contour::{contour_is_hole, Contour};
use self::svg::{generate_svg_with_metrics, generate_svg_with_trace_space, ColorRegion};

const HIGH_PRECISION_TRACE_SCALE: u32 = 2;
const ADAPTIVE_HIGH_DETAIL_MIN_REGION_AREA_FLOOR: f64 = 2.0;
const ADAPTIVE_HIGH_DETAIL_MIN_CONTOURS: usize = 1_024;
const ADAPTIVE_HIGH_DETAIL_TRACE_PIXELS_PER_CONTOUR: u64 = 320;
const ADAPTIVE_HIGH_DETAIL_PALETTE_MIN: usize = 16;
const ADAPTIVE_HIGH_DETAIL_COLOR_COUNT_MIN: u16 = 32;

/// Run the complete vectorization pipeline on a decoded image.
///
/// Returns the SVG as a `String`.
pub fn run_pipeline(img: &DynamicImage, config: &TracingConfig) -> Result<String> {
    Ok(run_pipeline_with_debug(img, config)?.into_svg())
}

/// Run the complete vectorization pipeline and keep debug-oriented stage data.
pub fn run_pipeline_with_debug(
    img: &DynamicImage,
    config: &TracingConfig,
) -> Result<TracingResult> {
    let output_width = img.width();
    let output_height = img.height();

    debug!(
        "Pipeline: preprocessing image ({}×{})",
        output_width, output_height
    );

    // Stage 1: Preprocessing (normalization, optional denoising)
    let preprocessed = preprocess::preprocess(img, config);

    // Stage 2: Composite transparency against the configured background color
    let bg = config.background_color.unwrap_or((255, 255, 255));
    let composited =
        preprocess::composite_against_background(&preprocessed, config.alpha_threshold, bg);

    let trace_scale = tracing_scale(config);
    let trace_image = if trace_scale > 1 {
        debug!(
            "Pipeline: resampling image to a {}× tracing grid for higher precision",
            trace_scale
        );
        preprocess::resample_for_tracing(&composited, trace_scale)
    } else {
        composited
    };

    // Stage 3: Color quantization / segmentation
    debug!(
        "Pipeline: quantizing colors (target: {})",
        config.color_count
    );
    let segmentation = segment::quantize(&trace_image, config);

    // Stage 4: Contour extraction
    debug!("Pipeline: extracting contours");
    let contour_extraction = contour::extract_contours_with_stats(&segmentation);
    let extracted_metrics = summarize_contours(&contour_extraction.contour_groups);

    // Stage 5: Despeckle – remove tiny contours below perimeter threshold
    let contour_groups = despeckle(
        contour_extraction.contour_groups,
        config.despeckle_threshold,
    );
    let despeckled_metrics = summarize_contours(&contour_groups);

    // Stage 6: Build color regions for SVG generation
    let trace_width = segmentation.width;
    let trace_height = segmentation.height;
    let effective_min_region_area = effective_svg_min_region_area(
        config,
        segmentation.palette.len(),
        trace_width,
        trace_height,
        extracted_metrics.contours,
    );
    let mut svg_config = config.clone();
    if effective_min_region_area > config.min_region_area {
        debug!(
            "Pipeline: raising effective SVG minimum region area from {:.2} to {:.2} for a dense high-detail trace",
            config.min_region_area,
            effective_min_region_area,
        );
        svg_config.min_region_area = effective_min_region_area;
    }

    let regions: Vec<ColorRegion> = contour_groups
        .into_iter()
        .filter_map(|(color_idx, contours)| {
            let color = *segmentation.palette.get(color_idx as usize)?;
            Some(ColorRegion { color, contours })
        })
        .collect();

    debug!("Pipeline: generating SVG ({} color regions)", regions.len());

    // Stage 7: SVG generation (includes simplification + curve fitting)
    let debug = TraceDebugInfo::new(
        segmentation.palette.clone(),
        regions
            .iter()
            .map(|region| {
                TracedRegionSummary::new(
                    region.color,
                    region.contours.len(),
                    region
                        .contours
                        .iter()
                        .filter(|contour| contour_is_hole(contour))
                        .count(),
                    region.contours.iter().map(std::vec::Vec::len).sum(),
                )
            })
            .collect(),
    );
    let svg_result = if trace_scale > 1 {
        generate_svg_with_trace_space(
            &regions,
            trace_width,
            trace_height,
            output_width,
            output_height,
            1.0 / trace_scale as f64,
            &svg_config,
        )
    } else {
        generate_svg_with_metrics(&regions, trace_width, trace_height, &svg_config)
    };
    let stage_metrics = TraceStageMetrics::with_svg_diagnostics(
        extracted_metrics.contours,
        extracted_metrics.holes,
        extracted_metrics.points,
        contour_extraction.invalid_contours_discarded,
        despeckled_metrics.contours,
        despeckled_metrics.holes,
        despeckled_metrics.points,
        svg_result.metrics.contours_simplified_away,
        svg_result.metrics.contours_filtered_min_area,
        svg_result.metrics.contours_suppressed_background,
        svg_result.metrics.contours_emitted,
        svg_result.metrics.holes_emitted,
        svg_result.metrics.points_emitted,
        svg_result.metrics.regions_emitted,
    );

    Ok(TracingResult::with_stage_metrics(
        svg_result.svg,
        output_width,
        output_height,
        debug,
        stage_metrics,
    ))
}

fn tracing_scale(config: &TracingConfig) -> u32 {
    if matches!(config.quality_preset, QualityPreset::High) {
        HIGH_PRECISION_TRACE_SCALE
    } else {
        1
    }
}

fn effective_svg_min_region_area(
    config: &TracingConfig,
    palette_len: usize,
    trace_width: u32,
    trace_height: u32,
    extracted_contours: usize,
) -> f64 {
    if should_raise_high_detail_min_region_area(
        config,
        palette_len,
        trace_width,
        trace_height,
        extracted_contours,
    ) {
        ADAPTIVE_HIGH_DETAIL_MIN_REGION_AREA_FLOOR.max(config.min_region_area)
    } else {
        config.min_region_area
    }
}

fn should_raise_high_detail_min_region_area(
    config: &TracingConfig,
    palette_len: usize,
    trace_width: u32,
    trace_height: u32,
    extracted_contours: usize,
) -> bool {
    if !matches!(config.quality_preset, QualityPreset::High)
        || !config.enable_preprocessing
        || !config.enable_denoising
        || config.color_count < ADAPTIVE_HIGH_DETAIL_COLOR_COUNT_MIN
        || palette_len < ADAPTIVE_HIGH_DETAIL_PALETTE_MIN
        || !uses_default_high_min_region_area(config)
        || extracted_contours < ADAPTIVE_HIGH_DETAIL_MIN_CONTOURS
    {
        return false;
    }

    let trace_pixel_count = u64::from(trace_width) * u64::from(trace_height);
    (extracted_contours as u64) * ADAPTIVE_HIGH_DETAIL_TRACE_PIXELS_PER_CONTOUR >= trace_pixel_count
}

fn uses_default_high_min_region_area(config: &TracingConfig) -> bool {
    let default_high = QualityPreset::High.to_config();
    (config.min_region_area - default_high.min_region_area).abs() <= f64::EPSILON
}

/// Remove contours whose perimeter is below the despeckle threshold.
///
/// The perimeter is approximated as the number of points in the contour
/// (i.e., the boundary pixel count, which is a good proxy for small speckles).
fn despeckle(contour_groups: Vec<(u8, Vec<Contour>)>, threshold: f64) -> Vec<(u8, Vec<Contour>)> {
    if threshold <= 0.0 {
        return contour_groups;
    }

    contour_groups
        .into_iter()
        .filter_map(|(color_idx, contours)| {
            let filtered: Vec<Contour> = contours
                .into_iter()
                .filter(|c| contour_perimeter(c) >= threshold)
                .collect();
            if filtered.is_empty() {
                None
            } else {
                Some((color_idx, filtered))
            }
        })
        .collect()
}

/// Approximate the perimeter of a contour by summing the distances
/// between consecutive points.
fn contour_perimeter(contour: &Contour) -> f64 {
    if contour.len() < 2 {
        return 0.0;
    }
    let mut perimeter = 0.0;
    for i in 0..contour.len() {
        let j = (i + 1) % contour.len();
        let dx = (contour[j].x - contour[i].x) as f64;
        let dy = (contour[j].y - contour[i].y) as f64;
        perimeter += (dx * dx + dy * dy).sqrt();
    }
    perimeter
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ContourStageSummary {
    contours: usize,
    holes: usize,
    points: usize,
}

fn summarize_contours(contour_groups: &[(u8, Vec<Contour>)]) -> ContourStageSummary {
    let mut summary = ContourStageSummary::default();

    for (_, contours) in contour_groups {
        summary.contours += contours.len();
        summary.holes += contours
            .iter()
            .filter(|contour| contour_is_hole(contour))
            .count();
        summary.points += contours.iter().map(std::vec::Vec::len).sum::<usize>();
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contour::Point;

    #[test]
    fn despeckle_removes_tiny_contours() {
        let small = vec![Point::new(0, 0), Point::new(1, 0), Point::new(0, 1)];
        let large: Vec<Point> = (0..20).map(|i| Point::new(i, 0)).collect();

        let groups = vec![(0u8, vec![small, large])];
        let result = despeckle(groups, 10.0);

        assert_eq!(result.len(), 1);
        // Only the large contour survives
        assert_eq!(result[0].1.len(), 1);
    }

    #[test]
    fn despeckle_zero_threshold_keeps_all() {
        let small = vec![Point::new(0, 0), Point::new(1, 0)];
        let groups = vec![(0u8, vec![small])];
        let result = despeckle(groups, 0.0);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn contour_perimeter_square() {
        let pts = vec![
            Point::new(0, 0),
            Point::new(10, 0),
            Point::new(10, 10),
            Point::new(0, 10),
        ];
        let p = contour_perimeter(&pts);
        assert!((p - 40.0).abs() < 1e-6);
    }

    #[test]
    fn summarize_contours_counts_holes_and_points() {
        let groups = vec![(
            0u8,
            vec![
                vec![
                    Point::new(0, 0),
                    Point::new(4, 0),
                    Point::new(4, 4),
                    Point::new(0, 4),
                ],
                vec![
                    Point::new(1, 1),
                    Point::new(1, 3),
                    Point::new(3, 3),
                    Point::new(3, 1),
                ],
            ],
        )];

        let summary = summarize_contours(&groups);
        assert_eq!(summary.contours, 2);
        assert_eq!(summary.holes, 1);
        assert_eq!(summary.points, 8);
    }

    #[test]
    fn tracing_scale_uses_dense_grid_for_high_preset() {
        assert_eq!(tracing_scale(&QualityPreset::Balanced.to_config()), 1);
        assert_eq!(tracing_scale(&QualityPreset::High.to_config()), 2);
    }

    #[test]
    fn effective_svg_min_region_area_raises_floor_for_dense_high_detail_traces() {
        let config = QualityPreset::High.to_config();

        assert_eq!(
            effective_svg_min_region_area(&config, 24, 1_536, 1_536, 8_740),
            2.0
        );
    }

    #[test]
    fn effective_svg_min_region_area_preserves_custom_or_low_density_configs() {
        let high = QualityPreset::High.to_config();
        let mut custom_high = high.clone();
        custom_high.min_region_area = 1.0;

        assert_eq!(
            effective_svg_min_region_area(&custom_high, 24, 1_536, 1_536, 8_740),
            1.0
        );
        assert_eq!(
            effective_svg_min_region_area(&high, 8, 1_536, 1_536, 8_740),
            0.5
        );
        assert_eq!(effective_svg_min_region_area(&high, 24, 512, 512, 200), 0.5);
        assert_eq!(
            effective_svg_min_region_area(
                &QualityPreset::Balanced.to_config(),
                24,
                1_536,
                1_536,
                8_740
            ),
            QualityPreset::Balanced.to_config().min_region_area
        );
    }
}
