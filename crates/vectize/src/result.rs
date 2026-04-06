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

/// Rich tracing result containing the SVG plus optional inspection data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TracingResult {
    svg: String,
    width: u32,
    height: u32,
    debug: TraceDebugInfo,
}

impl TracingResult {
    /// Create a new tracing result.
    pub fn new(svg: String, width: u32, height: u32, debug: TraceDebugInfo) -> Self {
        Self {
            svg,
            width,
            height,
            debug,
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
