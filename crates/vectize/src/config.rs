//! Configuration types and quality presets for the tracing pipeline.

/// Quality preset for controlling the speed/quality tradeoff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QualityPreset {
    /// Fast mode: lower quality, faster processing.
    Fast,
    /// Balanced mode: good quality with reasonable speed. (default)
    #[default]
    Balanced,
    /// High mode: best quality, slower processing.
    High,
}

impl QualityPreset {
    /// Returns the `TracingConfig` corresponding to this preset.
    pub fn to_config(self) -> TracingConfig {
        match self {
            QualityPreset::Fast => TracingConfig {
                color_count: 8,
                simplification_tolerance: 2.0,
                min_region_area: 16.0,
                smoothing_strength: 0.3,
                corner_sensitivity: 0.8,
                alpha_threshold: 128,
                despeckle_threshold: 4.0,
                enable_denoising: false,
                enable_preprocessing: true,
                quality_preset: QualityPreset::Fast,
                background_color: None,
            },
            QualityPreset::Balanced => TracingConfig::default(),
            QualityPreset::High => TracingConfig {
                color_count: 32,
                simplification_tolerance: 0.3,
                min_region_area: 2.0,
                smoothing_strength: 0.6,
                corner_sensitivity: 0.4,
                alpha_threshold: 64,
                despeckle_threshold: 1.0,
                enable_denoising: true,
                enable_preprocessing: true,
                quality_preset: QualityPreset::High,
                background_color: None,
            },
        }
    }

    /// Parse a quality preset from a string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "fast" => Some(QualityPreset::Fast),
            "balanced" => Some(QualityPreset::Balanced),
            "high" => Some(QualityPreset::High),
            _ => None,
        }
    }
}

impl std::fmt::Display for QualityPreset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QualityPreset::Fast => write!(f, "fast"),
            QualityPreset::Balanced => write!(f, "balanced"),
            QualityPreset::High => write!(f, "high"),
        }
    }
}

/// Complete tracing configuration controlling all pipeline parameters.
#[derive(Debug, Clone)]
pub struct TracingConfig {
    /// Number of colors in the output palette (2–256).
    pub color_count: u16,
    /// Polygon simplification tolerance in pixels (Ramer-Douglas-Peucker).
    pub simplification_tolerance: f64,
    /// Minimum region area in pixels; smaller regions are suppressed.
    pub min_region_area: f64,
    /// Curve smoothing strength (0.0 = no smoothing, 1.0 = maximum).
    pub smoothing_strength: f64,
    /// Corner sensitivity (0.0 = very smooth corners, 1.0 = preserve all corners).
    pub corner_sensitivity: f64,
    /// Alpha threshold for transparency (0–255); pixels below are treated as transparent.
    pub alpha_threshold: u8,
    /// Despeckle threshold: minimum contour perimeter to keep.
    pub despeckle_threshold: f64,
    /// Enable Gaussian denoising before tracing.
    pub enable_denoising: bool,
    /// Enable image preprocessing (normalization, contrast adjustment).
    pub enable_preprocessing: bool,
    /// The quality preset this config was derived from (if any).
    pub quality_preset: QualityPreset,
    /// Background color for alpha compositing and redundant-region suppression.
    /// `None` means white (255, 255, 255).
    pub background_color: Option<(u8, u8, u8)>,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            color_count: 16,
            simplification_tolerance: 1.0,
            min_region_area: 4.0,
            smoothing_strength: 0.5,
            corner_sensitivity: 0.6,
            alpha_threshold: 128,
            despeckle_threshold: 2.0,
            enable_denoising: false,
            enable_preprocessing: true,
            quality_preset: QualityPreset::Balanced,
            background_color: None,
        }
    }
}

impl TracingConfig {
    /// Validate the configuration, returning an error message if invalid.
    pub fn validate(&self) -> Result<(), String> {
        if !(2..=256).contains(&self.color_count) {
            return Err("color_count must be in the range 2..=256".to_string());
        }
        if self.simplification_tolerance < 0.0 {
            return Err("simplification_tolerance must be non-negative".to_string());
        }
        if self.min_region_area < 0.0 {
            return Err("min_region_area must be non-negative".to_string());
        }
        if !(0.0..=1.0).contains(&self.smoothing_strength) {
            return Err("smoothing_strength must be in [0.0, 1.0]".to_string());
        }
        if !(0.0..=1.0).contains(&self.corner_sensitivity) {
            return Err("corner_sensitivity must be in [0.0, 1.0]".to_string());
        }
        Ok(())
    }
}
