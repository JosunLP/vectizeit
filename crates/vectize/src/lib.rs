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
//! 4. **Contour extraction** – trace boundary pixels using Moore neighbor tracing
//! 5. **Simplification** – reduce polygon complexity with Ramer-Douglas-Peucker
//! 6. **Curve fitting** – smooth polylines with cubic Bezier splines
//! 7. **SVG generation** – emit clean, valid SVG markup
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use vectize::{Tracer, TracingConfig, QualityPreset};
//!
//! // Use a quality preset
//! let config = QualityPreset::High.to_config();
//! let tracer = Tracer::new(config);
//! let svg = tracer.trace_file("input.png").unwrap();
//! std::fs::write("output.svg", svg).unwrap();
//! ```

pub mod config;
pub mod error;
pub mod pipeline;

pub use config::{QualityPreset, TracingConfig};
pub use error::{Result, VectizeError};

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
        let img = pipeline::loader::load_from_file(path.as_ref())?;
        pipeline::run_pipeline(&img, &self.config)
    }

    /// Trace an image from raw bytes and return the SVG as a string.
    ///
    /// The image format is inferred from the byte magic header.
    ///
    /// # Errors
    /// Returns an error if the bytes cannot be decoded or traced.
    pub fn trace_bytes(&self, bytes: &[u8]) -> Result<String> {
        let img = pipeline::loader::load_from_bytes(bytes)?;
        pipeline::run_pipeline(&img, &self.config)
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
        let mut config = TracingConfig::default();
        config.color_count = 1;
        assert!(config.validate().is_err());
    }

    #[test]
    fn trace_bytes_invalid_data_returns_error() {
        let result = trace_bytes(b"not an image");
        assert!(result.is_err());
    }
}
