//! The multi-stage vectorization pipeline.
//!
//! Orchestrates all processing stages from raw image input to SVG output.

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

    // Stage 1: Preprocessing
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

    // Stage 5: Build color regions for SVG generation
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

    // Stage 6: SVG generation (stages 5–6 include simplification + curve fitting)
    let svg = svg::generate_svg(&regions, width, height, config);

    Ok(svg)
}
