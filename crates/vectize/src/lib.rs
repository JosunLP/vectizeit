//! # vectize
//!
//! High-quality raster-to-vector image tracing library.
//!
//! ## Overview
//!
//! `vectize` converts bitmap images (PNG, JPEG, WebP) into clean SVG vector graphics
//! through a multi-stage processing pipeline:
//!
//! 1. **Loading** – decode the source image
//! 2. **Preprocessing** – normalize, optionally denoise, handle transparency
//! 3. **Segmentation** – reduce colors via median-cut quantization
//! 4. **Contour extraction** – trace deterministic grid-edge loops with hole preservation
//! 5. **Simplification** – reduce polygon complexity with Ramer-Douglas-Peucker
//! 6. **Contour smoothing + curve fitting** – adaptively smooth closed contours,
//!    then fit corner-aware cubic Bezier splines
//! 7. **SVG generation** – emit clean, valid SVG markup
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use vectize::{QualityPreset, Tracer};
//!
//! // Use a quality preset
//! let tracer = Tracer::with_preset(QualityPreset::High);
//! let result = tracer.trace_file_result("input.png").unwrap();
//! result.write_svg("output.svg", true).unwrap();
//! ```

pub mod config;
pub mod error;
pub mod pipeline;
pub mod result;

pub use config::{QualityPreset, TracingConfig};
pub use error::{Result, VectizeError};
pub use pipeline::segment::PaletteColor;
pub use result::{TraceDebugInfo, TraceStageMetrics, TracedRegionSummary, TracingResult};

use std::path::Path;

/// The main entry point for the tracing pipeline.
///
/// `Tracer` holds a [`TracingConfig`] and provides methods to trace images
/// from files or raw bytes.
#[derive(Debug, Clone)]
pub struct Tracer {
    config: TracingConfig,
}

impl Tracer {
    /// Create a new `Tracer` with the given configuration.
    pub fn new(config: TracingConfig) -> Self {
        Self { config }
    }

    /// Create a `Tracer` using the given quality preset.
    pub fn with_preset(preset: QualityPreset) -> Self {
        Self::new(preset.to_config())
    }

    /// Trace an image file and return the SVG as a string.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read, decoded, or traced.
    pub fn trace_file(&self, path: impl AsRef<Path>) -> Result<String> {
        Ok(self.trace_file_result(path)?.into_svg())
    }

    /// Trace an image from raw bytes and return the SVG as a string.
    ///
    /// The image format is inferred from the byte magic header.
    ///
    /// # Errors
    /// Returns an error if the bytes cannot be decoded or traced.
    pub fn trace_bytes(&self, bytes: &[u8]) -> Result<String> {
        Ok(self.trace_bytes_result(bytes)?.into_svg())
    }

    /// Trace an image file and return a rich [`TracingResult`].
    ///
    /// This variant preserves debug-oriented data such as the quantized palette,
    /// contour summaries, and stage metrics for downstream inspection or tuning.
    pub fn trace_file_result(&self, path: impl AsRef<Path>) -> Result<TracingResult> {
        let img = pipeline::loader::load_from_file(path.as_ref())?;
        pipeline::run_pipeline_with_debug(&img, &self.config)
    }

    /// Trace an image from raw bytes and return a rich [`TracingResult`].
    pub fn trace_bytes_result(&self, bytes: &[u8]) -> Result<TracingResult> {
        let img = pipeline::loader::load_from_bytes(bytes)?;
        pipeline::run_pipeline_with_debug(&img, &self.config)
    }

    /// Return a reference to the current configuration.
    pub fn config(&self) -> &TracingConfig {
        &self.config
    }
}

/// Convenience function: trace a file with default settings and return the SVG.
///
/// Equivalent to `Tracer::with_preset(QualityPreset::Balanced).trace_file(path)`.
pub fn trace_file(path: impl AsRef<Path>) -> Result<String> {
    Tracer::with_preset(QualityPreset::Balanced).trace_file(path)
}

/// Convenience function: trace image bytes with default settings and return the SVG.
pub fn trace_bytes(bytes: &[u8]) -> Result<String> {
    Tracer::with_preset(QualityPreset::Balanced).trace_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracer_default_config() {
        let tracer = Tracer::with_preset(QualityPreset::Balanced);
        let config = tracer.config();
        assert_eq!(config.color_count, 16);
    }

    #[test]
    fn tracer_high_preset() {
        let tracer = Tracer::with_preset(QualityPreset::High);
        let config = tracer.config();
        assert_eq!(config.color_count, 32);
        assert!(config.enable_denoising);
    }

    #[test]
    fn config_validation_passes_for_default() {
        let config = TracingConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn config_validation_rejects_zero_colors() {
        let config = TracingConfig {
            color_count: 1,
            ..TracingConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn trace_bytes_result_exposes_debug_info() {
        let tracer = Tracer::with_preset(QualityPreset::Balanced);
        let png = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(png)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();

        let result = tracer.trace_bytes_result(&buf.into_inner()).unwrap();
        assert_eq!(result.width(), 2);
        assert_eq!(result.height(), 2);
        assert!(!result.debug().palette().is_empty());
        assert!(result.stage_metrics().is_some());
    }

    #[test]
    fn trace_bytes_invalid_data_returns_error() {
        let result = trace_bytes(b"not an image");
        assert!(result.is_err());
    }
}
