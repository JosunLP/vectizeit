//! Configuration types and quality presets for the tracing pipeline.

/// The implicit background color used when no custom background is configured.
pub const DEFAULT_BACKGROUND_COLOR: (u8, u8, u8) = (255, 255, 255);

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

/// Partial tracing configuration updates applied on top of a preset or existing config.
///
/// This is useful for frontends such as the CLI, wasm bindings, or application code that wants
/// to start from a quality preset and override only a subset of fields.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TracingConfigOverrides {
    /// Override the number of palette colors.
    pub color_count: Option<u16>,
    /// Override the polygon simplification tolerance.
    pub simplification_tolerance: Option<f64>,
    /// Override the minimum emitted region area.
    pub min_region_area: Option<f64>,
    /// Override the contour smoothing strength.
    pub smoothing_strength: Option<f64>,
    /// Override the corner preservation strength.
    pub corner_sensitivity: Option<f64>,
    /// Override the alpha threshold.
    pub alpha_threshold: Option<u8>,
    /// Override the despeckle threshold.
    pub despeckle_threshold: Option<f64>,
    /// Override whether denoising is enabled.
    pub enable_denoising: Option<bool>,
    /// Override whether preprocessing is enabled.
    pub enable_preprocessing: Option<bool>,
    /// Override the compositing / SVG background color.
    pub background_color: Option<(u8, u8, u8)>,
    /// Override whether SVG gradients are enabled.
    pub enable_svg_gradients: Option<bool>,
    /// Override the tile size used for tile-aware segmentation.
    pub tile_size: Option<u32>,
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
                enable_svg_gradients: false,
                tile_size: None,
            },
            QualityPreset::Balanced => TracingConfig::default(),
            QualityPreset::High => TracingConfig {
                color_count: 64,
                simplification_tolerance: 1.2,
                min_region_area: 0.5,
                smoothing_strength: 0.35,
                corner_sensitivity: 0.9,
                alpha_threshold: 48,
                despeckle_threshold: 0.0,
                enable_denoising: true,
                enable_preprocessing: true,
                quality_preset: QualityPreset::High,
                background_color: None,
                enable_svg_gradients: false,
                tile_size: None,
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
    /// Corner preservation strength for adaptive smoothing and Bezier fitting
    /// (0.0 = smooth all corners, 1.0 = preserve sharp corners).
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
    /// Enable SVG gradient approximation for smoothly varying regions.
    pub enable_svg_gradients: bool,
    /// Optional tile size used for tile-aware segmentation on large images.
    /// `None` keeps the original full-frame segmentation path.
    pub tile_size: Option<u32>,
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
            enable_svg_gradients: false,
            tile_size: None,
        }
    }
}

impl TracingConfig {
    /// Build a validated config from a quality preset plus a set of overrides.
    pub fn from_preset_with_overrides(
        preset: QualityPreset,
        overrides: &TracingConfigOverrides,
    ) -> Result<Self, String> {
        let mut config = preset.to_config();
        config.apply_overrides(overrides);
        config.validate()?;
        Ok(config)
    }

    /// Apply a partial set of overrides to the current config.
    pub fn apply_overrides(&mut self, overrides: &TracingConfigOverrides) {
        if let Some(value) = overrides.color_count {
            self.color_count = value;
        }
        if let Some(value) = overrides.simplification_tolerance {
            self.simplification_tolerance = value;
        }
        if let Some(value) = overrides.min_region_area {
            self.min_region_area = value;
        }
        if let Some(value) = overrides.smoothing_strength {
            self.smoothing_strength = value;
        }
        if let Some(value) = overrides.corner_sensitivity {
            self.corner_sensitivity = value;
        }
        if let Some(value) = overrides.alpha_threshold {
            self.alpha_threshold = value;
        }
        if let Some(value) = overrides.despeckle_threshold {
            self.despeckle_threshold = value;
        }
        if let Some(value) = overrides.enable_denoising {
            self.enable_denoising = value;
        }
        if let Some(value) = overrides.enable_preprocessing {
            self.enable_preprocessing = value;
        }
        if let Some(value) = overrides.background_color {
            self.background_color = Some(value);
        }
        if let Some(value) = overrides.enable_svg_gradients {
            self.enable_svg_gradients = value;
        }
        if let Some(value) = overrides.tile_size {
            self.tile_size = Some(value);
        }
    }

    /// Resolve the effective background color for compositing and SVG emission.
    pub fn resolved_background_color(&self) -> (u8, u8, u8) {
        self.background_color.unwrap_or(DEFAULT_BACKGROUND_COLOR)
    }

    /// Parse a `#rrggbb` or `rrggbb` color string into an RGB tuple.
    pub fn parse_hex_color(value: &str) -> Result<(u8, u8, u8), String> {
        let value = value.trim();
        let hex = value.strip_prefix('#').unwrap_or(value);

        if hex.len() != 6 {
            return Err(format!(
                "Invalid hex color '{value}': expected 6 hex digits (e.g. \"#ff0000\")"
            ));
        }

        let r = u8::from_str_radix(&hex[0..2], 16)
            .map_err(|_| format!("Invalid hex color '{value}'"))?;
        let g = u8::from_str_radix(&hex[2..4], 16)
            .map_err(|_| format!("Invalid hex color '{value}'"))?;
        let b = u8::from_str_radix(&hex[4..6], 16)
            .map_err(|_| format!("Invalid hex color '{value}'"))?;

        Ok((r, g, b))
    }

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
        if self.tile_size.is_some_and(|tile_size| tile_size < 2) {
            return Err("tile_size must be at least 2 pixels when set".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_background_color_defaults_to_white() {
        assert_eq!(
            TracingConfig::default().resolved_background_color(),
            (255, 255, 255)
        );
    }

    #[test]
    fn resolved_background_color_prefers_configured_value() {
        let config = TracingConfig {
            background_color: Some((0x12, 0x34, 0x56)),
            ..TracingConfig::default()
        };

        assert_eq!(config.resolved_background_color(), (0x12, 0x34, 0x56));
    }

    #[test]
    fn parse_hex_color_accepts_prefixed_and_unprefixed_values() {
        assert_eq!(
            TracingConfig::parse_hex_color("#123456").unwrap(),
            (0x12, 0x34, 0x56)
        );
        assert_eq!(
            TracingConfig::parse_hex_color("abcdef").unwrap(),
            (0xab, 0xcd, 0xef)
        );
    }

    #[test]
    fn parse_hex_color_rejects_invalid_values() {
        assert!(TracingConfig::parse_hex_color("#12345").is_err());
        assert!(TracingConfig::parse_hex_color("gg0000").is_err());
    }

    #[test]
    fn apply_overrides_updates_only_selected_fields() {
        let mut config = QualityPreset::Fast.to_config();
        config.apply_overrides(&TracingConfigOverrides {
            smoothing_strength: Some(0.75),
            background_color: Some((0x12, 0x34, 0x56)),
            enable_svg_gradients: Some(true),
            ..TracingConfigOverrides::default()
        });

        assert_eq!(
            config.color_count,
            QualityPreset::Fast.to_config().color_count
        );
        assert_eq!(config.smoothing_strength, 0.75);
        assert_eq!(config.background_color, Some((0x12, 0x34, 0x56)));
        assert!(config.enable_svg_gradients);
    }

    #[test]
    fn from_preset_with_overrides_applies_values_and_validates() {
        let config = TracingConfig::from_preset_with_overrides(
            QualityPreset::High,
            &TracingConfigOverrides {
                color_count: Some(24),
                enable_denoising: Some(false),
                enable_preprocessing: Some(false),
                tile_size: Some(128),
                ..TracingConfigOverrides::default()
            },
        )
        .unwrap();

        assert_eq!(config.color_count, 24);
        assert!(!config.enable_denoising);
        assert!(!config.enable_preprocessing);
        assert_eq!(config.tile_size, Some(128));
        assert!(matches!(config.quality_preset, QualityPreset::High));
    }

    #[test]
    fn from_preset_with_overrides_rejects_invalid_results() {
        let error = TracingConfig::from_preset_with_overrides(
            QualityPreset::Balanced,
            &TracingConfigOverrides {
                tile_size: Some(1),
                ..TracingConfigOverrides::default()
            },
        )
        .unwrap_err();

        assert!(error.contains("tile_size"));
    }
}
