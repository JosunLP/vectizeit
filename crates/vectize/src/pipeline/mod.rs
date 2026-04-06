//! The multi-stage vectorization pipeline.
//!
//! Orchestrates all processing stages from raw image input to SVG output.
//!
//! ## Pipeline Stages
//!
//! 1. **Preprocessing** – normalize to RGBA8, optionally denoise
//! 2. **Alpha compositing** – composite transparent pixels against white
//! 3. **Color quantization** – median-cut palette reduction
//! 4. **Contour extraction** – Moore neighbor boundary tracing
//! 5. **Despeckle** – remove tiny contours below the threshold
//! 6. **Region assembly** – build color regions from contour data
//! 7. **SVG generation** – simplification, curve fitting, and SVG emission

pub mod contour;
pub mod curves;
pub mod loader;
pub mod preprocess;
pub mod segment;
pub mod simplify;
pub mod svg;

use image::DynamicImage;
use log::debug;

use crate::config::TracingConfig;
use crate::error::Result;

use self::contour::Contour;
use self::svg::ColorRegion;

/// Run the complete vectorization pipeline on a decoded image.
///
/// Returns the SVG as a `String`.
pub fn run_pipeline(img: &DynamicImage, config: &TracingConfig) -> Result<String> {
    debug!(
        "Pipeline: preprocessing image ({}×{})",
        img.width(),
        img.height()
    );

    // Stage 1: Preprocessing (normalization, optional denoising)
    let preprocessed = preprocess::preprocess(img, config);

    // Stage 2: Composite transparency against white
    let composited = preprocess::composite_against_white(&preprocessed, config.alpha_threshold);

    // Stage 3: Color quantization / segmentation
    debug!(
        "Pipeline: quantizing colors (target: {})",
        config.color_count
    );
    let segmentation = segment::quantize(&composited, config.color_count, config.alpha_threshold);

    // Stage 4: Contour extraction
    debug!("Pipeline: extracting contours");
    let contour_groups = contour::extract_contours(&segmentation);

    // Stage 5: Despeckle – remove tiny contours below perimeter threshold
    let contour_groups = despeckle(contour_groups, config.despeckle_threshold);

    // Stage 6: Build color regions for SVG generation
    let width = segmentation.width;
    let height = segmentation.height;

    let regions: Vec<ColorRegion> = contour_groups
        .into_iter()
        .filter_map(|(color_idx, contours)| {
            let color = *segmentation.palette.get(color_idx as usize)?;
            Some(ColorRegion { color, contours })
        })
        .collect();

    debug!("Pipeline: generating SVG ({} color regions)", regions.len());

    // Stage 7: SVG generation (includes simplification + curve fitting)
    let svg = svg::generate_svg(&regions, width, height, config);

    Ok(svg)
}

/// Remove contours whose perimeter is below the despeckle threshold.
///
/// The perimeter is approximated as the number of points in the contour
/// (i.e., the boundary pixel count, which is a good proxy for small speckles).
fn despeckle(
    contour_groups: Vec<(u8, Vec<Contour>)>,
    threshold: f64,
) -> Vec<(u8, Vec<Contour>)> {
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
}
