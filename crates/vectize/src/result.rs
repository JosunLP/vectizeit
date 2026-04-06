//! Public tracing result types.

use std::path::Path;

use crate::error::{Result, VectizeError};
use crate::pipeline::segment::PaletteColor;

/// Summary information about one traced color region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracedRegionSummary {
    color: PaletteColor,
    contour_count: usize,
    hole_count: usize,
    total_points: usize,
}

impl TracedRegionSummary {
    /// Create a new region summary.
    pub fn new(
        color: PaletteColor,
        contour_count: usize,
        hole_count: usize,
        total_points: usize,
    ) -> Self {
        Self {
            color,
            contour_count,
            hole_count,
            total_points,
        }
    }

    /// Return the palette color assigned to this region.
    pub fn color(&self) -> PaletteColor {
        self.color
    }

    /// Return the number of contours in this region.
    pub fn contour_count(&self) -> usize {
        self.contour_count
    }

    /// Return the number of interior hole contours in this region.
    pub fn hole_count(&self) -> usize {
        self.hole_count
    }

    /// Return the total traced point count across all contours.
    pub fn total_points(&self) -> usize {
        self.total_points
    }
}

/// Optional debug-oriented information captured during tracing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceDebugInfo {
    palette: Vec<PaletteColor>,
    regions: Vec<TracedRegionSummary>,
}

impl TraceDebugInfo {
    /// Create a new debug info object.
    pub fn new(palette: Vec<PaletteColor>, regions: Vec<TracedRegionSummary>) -> Self {
        Self { palette, regions }
    }

    /// Return the quantized palette used during tracing.
    pub fn palette(&self) -> &[PaletteColor] {
        &self.palette
    }

    /// Return summaries for the traced color regions.
    pub fn regions(&self) -> &[TracedRegionSummary] {
        &self.regions
    }
}

/// Structured stage metrics captured during a tracing run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceStageMetrics {
    contours_extracted: usize,
    holes_extracted: usize,
    points_extracted: usize,
    invalid_contours_discarded: usize,
    contours_after_despeckle: usize,
    holes_after_despeckle: usize,
    points_after_despeckle: usize,
    contours_simplified_away: usize,
    contours_filtered_min_area: usize,
    contours_suppressed_background: usize,
    contours_emitted: usize,
    holes_emitted: usize,
    points_emitted: usize,
    regions_emitted: usize,
}

impl TraceStageMetrics {
    /// Create a new stage metrics object.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        contours_extracted: usize,
        holes_extracted: usize,
        points_extracted: usize,
        contours_after_despeckle: usize,
        holes_after_despeckle: usize,
        points_after_despeckle: usize,
        contours_emitted: usize,
        holes_emitted: usize,
        points_emitted: usize,
        regions_emitted: usize,
    ) -> Self {
        Self::from_parts(
            contours_extracted,
            holes_extracted,
            points_extracted,
            0,
            contours_after_despeckle,
            holes_after_despeckle,
            points_after_despeckle,
            0,
            0,
            0,
            contours_emitted,
            holes_emitted,
            points_emitted,
            regions_emitted,
        )
    }

    /// Create a new stage metrics object with contour extraction diagnostics.
    #[allow(clippy::too_many_arguments)]
    pub fn with_invalid_contours_discarded(
        contours_extracted: usize,
        holes_extracted: usize,
        points_extracted: usize,
        invalid_contours_discarded: usize,
        contours_after_despeckle: usize,
        holes_after_despeckle: usize,
        points_after_despeckle: usize,
        contours_emitted: usize,
        holes_emitted: usize,
        points_emitted: usize,
        regions_emitted: usize,
    ) -> Self {
        Self::from_parts(
            contours_extracted,
            holes_extracted,
            points_extracted,
            invalid_contours_discarded,
            contours_after_despeckle,
            holes_after_despeckle,
            points_after_despeckle,
            0,
            0,
            0,
            contours_emitted,
            holes_emitted,
            points_emitted,
            regions_emitted,
        )
    }

    /// Create a new stage metrics object with contour extraction and SVG filtering diagnostics.
    #[allow(clippy::too_many_arguments)]
    pub fn with_svg_diagnostics(
        contours_extracted: usize,
        holes_extracted: usize,
        points_extracted: usize,
        invalid_contours_discarded: usize,
        contours_after_despeckle: usize,
        holes_after_despeckle: usize,
        points_after_despeckle: usize,
        contours_simplified_away: usize,
        contours_filtered_min_area: usize,
        contours_suppressed_background: usize,
        contours_emitted: usize,
        holes_emitted: usize,
        points_emitted: usize,
        regions_emitted: usize,
    ) -> Self {
        Self::from_parts(
            contours_extracted,
            holes_extracted,
            points_extracted,
            invalid_contours_discarded,
            contours_after_despeckle,
            holes_after_despeckle,
            points_after_despeckle,
            contours_simplified_away,
            contours_filtered_min_area,
            contours_suppressed_background,
            contours_emitted,
            holes_emitted,
            points_emitted,
            regions_emitted,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_parts(
        contours_extracted: usize,
        holes_extracted: usize,
        points_extracted: usize,
        invalid_contours_discarded: usize,
        contours_after_despeckle: usize,
        holes_after_despeckle: usize,
        points_after_despeckle: usize,
        contours_simplified_away: usize,
        contours_filtered_min_area: usize,
        contours_suppressed_background: usize,
        contours_emitted: usize,
        holes_emitted: usize,
        points_emitted: usize,
        regions_emitted: usize,
    ) -> Self {
        Self {
            contours_extracted,
            holes_extracted,
            points_extracted,
            invalid_contours_discarded,
            contours_after_despeckle,
            holes_after_despeckle,
            points_after_despeckle,
            contours_simplified_away,
            contours_filtered_min_area,
            contours_suppressed_background,
            contours_emitted,
            holes_emitted,
            points_emitted,
            regions_emitted,
        }
    }

    /// Return the contour count immediately after contour extraction.
    pub fn contours_extracted(&self) -> usize {
        self.contours_extracted
    }

    /// Return the hole contour count immediately after contour extraction.
    pub fn holes_extracted(&self) -> usize {
        self.holes_extracted
    }

    /// Return the traced point count immediately after contour extraction.
    pub fn points_extracted(&self) -> usize {
        self.points_extracted
    }

    /// Return the number of invalid contour loops discarded during contour extraction.
    pub fn invalid_contours_discarded(&self) -> usize {
        self.invalid_contours_discarded
    }

    /// Return the contour count after despeckling.
    pub fn contours_after_despeckle(&self) -> usize {
        self.contours_after_despeckle
    }

    /// Return the hole contour count after despeckling.
    pub fn holes_after_despeckle(&self) -> usize {
        self.holes_after_despeckle
    }

    /// Return the traced point count after despeckling.
    pub fn points_after_despeckle(&self) -> usize {
        self.points_after_despeckle
    }

    /// Return the number of contours that collapsed below three vertices after SVG simplification.
    pub fn contours_simplified_away(&self) -> usize {
        self.contours_simplified_away
    }

    /// Return the number of contours filtered out by `min_region_area` during SVG generation.
    pub fn contours_filtered_min_area(&self) -> usize {
        self.contours_filtered_min_area
    }

    /// Return the number of redundant border-connected background contours suppressed during SVG generation.
    pub fn contours_suppressed_background(&self) -> usize {
        self.contours_suppressed_background
    }

    /// Return the contour count that survived SVG contour filtering.
    pub fn contours_emitted(&self) -> usize {
        self.contours_emitted
    }

    /// Return the hole contour count that survived SVG contour filtering.
    pub fn holes_emitted(&self) -> usize {
        self.holes_emitted
    }

    /// Return the total coordinate point count emitted into final SVG path data
    /// after simplification and optional smoothing.
    pub fn points_emitted(&self) -> usize {
        self.points_emitted
    }

    /// Return the number of `<path>` regions emitted into the SVG.
    pub fn regions_emitted(&self) -> usize {
        self.regions_emitted
    }
}

/// Rich tracing result containing the SVG plus optional inspection data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracingResult {
    svg: String,
    width: u32,
    height: u32,
    debug: TraceDebugInfo,
    stage_metrics: Option<TraceStageMetrics>,
}

impl TracingResult {
    /// Create a new tracing result.
    pub fn new(svg: String, width: u32, height: u32, debug: TraceDebugInfo) -> Self {
        Self {
            svg,
            width,
            height,
            debug,
            stage_metrics: None,
        }
    }

    /// Create a new tracing result with structured stage metrics.
    pub fn with_stage_metrics(
        svg: String,
        width: u32,
        height: u32,
        debug: TraceDebugInfo,
        stage_metrics: TraceStageMetrics,
    ) -> Self {
        Self {
            svg,
            width,
            height,
            debug,
            stage_metrics: Some(stage_metrics),
        }
    }

    /// Return the SVG markup.
    pub fn svg(&self) -> &str {
        &self.svg
    }

    /// Consume the result and return the SVG markup.
    pub fn into_svg(self) -> String {
        self.svg
    }

    /// Return the traced image width.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Return the traced image height.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Return debug-oriented tracing information.
    pub fn debug(&self) -> &TraceDebugInfo {
        &self.debug
    }

    /// Return structured metrics captured across tracing stages when available.
    pub fn stage_metrics(&self) -> Option<&TraceStageMetrics> {
        self.stage_metrics.as_ref()
    }

    /// Write the SVG to a file.
    ///
    /// When `overwrite` is `false`, an error is returned if the output path already exists.
    pub fn write_svg(&self, path: impl AsRef<Path>, overwrite: bool) -> Result<()> {
        let path = path.as_ref();
        if path.exists() && !overwrite {
            return Err(VectizeError::OutputExists(path.display().to_string()));
        }

        std::fs::write(path, self.svg())?;
        Ok(())
    }
}
