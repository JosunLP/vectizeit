//! Integration tests for the vectize library.
//!
//! These tests exercise the full public API end-to-end using
//! programmatically generated in-memory images.

use image::{ImageBuffer, ImageFormat, Rgba, RgbaImage};
use std::io::Cursor;
use vectize::{QualityPreset, Tracer, TracingConfig, VectizeError};

/// Helper: create a solid-color RGBA image of the given size and color.
fn make_solid_image(width: u32, height: u32, color: Rgba<u8>) -> RgbaImage {
    ImageBuffer::from_fn(width, height, |_, _| color)
}

/// Helper: encode an RGBA image to PNG bytes.
fn encode_png(img: &RgbaImage) -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, ImageFormat::Png).unwrap();
    buf.into_inner()
}

/// Helper: encode an RGBA image to JPEG bytes (converts to RGB first since JPEG has no alpha).
fn encode_jpeg(img: &RgbaImage) -> Vec<u8> {
    let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8();
    let mut buf = Cursor::new(Vec::new());
    rgb.write_to(&mut buf, ImageFormat::Jpeg).unwrap();
    buf.into_inner()
}

// ---------------------------------------------------------------------------
// API Integration Tests
// ---------------------------------------------------------------------------

#[test]
fn trace_bytes_png_solid_red() {
    let img = make_solid_image(16, 16, Rgba([255, 0, 0, 255]));
    let bytes = encode_png(&img);

    let tracer = Tracer::with_preset(QualityPreset::Fast);
    let svg = tracer.trace_bytes(&bytes).unwrap();

    assert!(svg.contains("<svg"));
    assert!(svg.contains("</svg>"));
    assert!(svg.contains("viewBox=\"0 0 16 16\""));
}

#[test]
fn trace_bytes_jpeg_solid_blue() {
    let img = make_solid_image(32, 32, Rgba([0, 0, 255, 255]));
    let bytes = encode_jpeg(&img);

    let tracer = Tracer::with_preset(QualityPreset::Balanced);
    let svg = tracer.trace_bytes(&bytes).unwrap();

    assert!(svg.contains("<svg"));
    assert!(svg.contains("</svg>"));
    assert!(svg.contains("viewBox=\"0 0 32 32\""));
}

#[test]
fn trace_bytes_two_color_image() {
    // Create a 16x16 image: left half red, right half green
    let img = ImageBuffer::from_fn(16, 16, |x, _| {
        if x < 8 {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([0, 255, 0, 255])
        }
    });
    let bytes = encode_png(&img);

    let tracer = Tracer::with_preset(QualityPreset::Balanced);
    let svg = tracer.trace_bytes(&bytes).unwrap();

    // Should produce at least two <path> elements for two color regions
    let path_count = svg.matches("<path").count();
    assert!(
        path_count >= 2,
        "Expected at least 2 path elements, got {path_count}"
    );
}

#[test]
fn trace_bytes_transparent_image() {
    // Fully transparent image
    let img = make_solid_image(8, 8, Rgba([0, 0, 0, 0]));
    let bytes = encode_png(&img);

    let tracer = Tracer::with_preset(QualityPreset::Fast);
    let svg = tracer.trace_bytes(&bytes).unwrap();

    // Should still produce valid SVG, even if content is essentially blank
    assert!(svg.contains("<svg"));
    assert!(svg.contains("</svg>"));
}

#[test]
fn trace_bytes_with_all_presets() {
    let img = ImageBuffer::from_fn(20, 20, |x, y| {
        if (x + y) % 2 == 0 {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([0, 0, 255, 255])
        }
    });
    let bytes = encode_png(&img);

    for preset in [
        QualityPreset::Fast,
        QualityPreset::Balanced,
        QualityPreset::High,
    ] {
        let tracer = Tracer::with_preset(preset);
        let svg = tracer.trace_bytes(&bytes).unwrap();
        assert!(
            svg.contains("<svg"),
            "Preset {preset} did not produce valid SVG"
        );
    }
}

#[test]
fn trace_bytes_custom_config() {
    let img = make_solid_image(10, 10, Rgba([128, 128, 128, 255]));
    let bytes = encode_png(&img);

    let config = TracingConfig {
        color_count: 4,
        simplification_tolerance: 0.5,
        min_region_area: 1.0,
        smoothing_strength: 0.3,
        corner_sensitivity: 0.8,
        alpha_threshold: 64,
        despeckle_threshold: 1.0,
        enable_denoising: true,
        enable_preprocessing: true,
        quality_preset: QualityPreset::High,
    };

    let tracer = Tracer::new(config);
    let svg = tracer.trace_bytes(&bytes).unwrap();
    assert!(svg.contains("<svg"));
}

#[test]
fn trace_bytes_result_exposes_debug_data() {
    let img = make_solid_image(12, 10, Rgba([20, 40, 60, 255]));
    let bytes = encode_png(&img);

    let tracer = Tracer::with_preset(QualityPreset::Balanced);
    let result = tracer.trace_bytes_result(&bytes).unwrap();

    assert_eq!(result.width(), 12);
    assert_eq!(result.height(), 10);
    assert!(result.svg().contains("<svg"));
    assert!(!result.debug().palette().is_empty());
    assert!(!result.debug().regions().is_empty());
}

#[test]
fn tracing_result_can_write_svg() {
    let img = make_solid_image(6, 6, Rgba([80, 90, 100, 255]));
    let bytes = encode_png(&img);
    let output_path = std::env::temp_dir().join("vectize_test_tracing_result.svg");
    let _ = std::fs::remove_file(&output_path);

    let tracer = Tracer::with_preset(QualityPreset::Balanced);
    let result = tracer.trace_bytes_result(&bytes).unwrap();
    result.write_svg(&output_path, false).unwrap();

    let written = std::fs::read_to_string(&output_path).unwrap();
    assert!(written.contains("<svg"));

    let _ = std::fs::remove_file(&output_path);
}

#[test]
fn trace_bytes_invalid_data_returns_error() {
    let tracer = Tracer::with_preset(QualityPreset::Balanced);
    let result = tracer.trace_bytes(b"not an image");
    assert!(result.is_err());
}

#[test]
fn trace_file_nonexistent_returns_error() {
    let tracer = Tracer::with_preset(QualityPreset::Balanced);
    let result = tracer.trace_file("/nonexistent/path.png");
    assert!(result.is_err());
}

#[test]
fn trace_file_round_trip() {
    // Create a temporary PNG file, trace it, write the SVG, verify output
    let img = make_solid_image(24, 24, Rgba([0, 200, 100, 255]));
    let png_path = std::env::temp_dir().join("vectize_test_roundtrip.png");
    let svg_path = std::env::temp_dir().join("vectize_test_roundtrip.svg");

    img.save(&png_path).unwrap();

    let tracer = Tracer::with_preset(QualityPreset::Balanced);
    let svg = tracer.trace_file(&png_path).unwrap();

    std::fs::write(&svg_path, &svg).unwrap();

    let written = std::fs::read_to_string(&svg_path).unwrap();
    assert!(written.contains("<svg"));
    assert!(written.contains("viewBox=\"0 0 24 24\""));

    // Cleanup
    let _ = std::fs::remove_file(&png_path);
    let _ = std::fs::remove_file(&svg_path);
}

#[test]
fn convenience_functions_work() {
    let img = make_solid_image(8, 8, Rgba([50, 100, 150, 255]));
    let bytes = encode_png(&img);

    let svg = vectize::trace_bytes(&bytes).unwrap();
    assert!(svg.contains("<svg"));

    let png_path = std::env::temp_dir().join("vectize_test_convenience.png");
    img.save(&png_path).unwrap();
    let svg = vectize::trace_file(&png_path).unwrap();
    assert!(svg.contains("<svg"));

    let _ = std::fs::remove_file(&png_path);
}

// ---------------------------------------------------------------------------
// Configuration Tests
// ---------------------------------------------------------------------------

#[test]
fn config_validation_comprehensive() {
    // Valid default
    assert!(TracingConfig::default().validate().is_ok());

    // Valid presets
    assert!(QualityPreset::Fast.to_config().validate().is_ok());
    assert!(QualityPreset::Balanced.to_config().validate().is_ok());
    assert!(QualityPreset::High.to_config().validate().is_ok());

    // Invalid: color_count too low
    let mut cfg = TracingConfig::default();
    cfg.color_count = 0;
    assert!(cfg.validate().is_err());

    cfg.color_count = 1;
    assert!(cfg.validate().is_err());

    cfg.color_count = 256;
    assert!(cfg.validate().is_ok());

    cfg.color_count = 257;
    assert!(cfg.validate().is_err());

    // Invalid: negative tolerance
    let mut cfg = TracingConfig::default();
    cfg.simplification_tolerance = -0.1;
    assert!(cfg.validate().is_err());

    // Invalid: negative min_region_area
    let mut cfg = TracingConfig::default();
    cfg.min_region_area = -1.0;
    assert!(cfg.validate().is_err());

    // Invalid: smoothing out of range
    let mut cfg = TracingConfig::default();
    cfg.smoothing_strength = 1.5;
    assert!(cfg.validate().is_err());
    cfg.smoothing_strength = -0.1;
    assert!(cfg.validate().is_err());

    // Invalid: corner sensitivity out of range
    let mut cfg = TracingConfig::default();
    cfg.corner_sensitivity = 2.0;
    assert!(cfg.validate().is_err());
    cfg.corner_sensitivity = -0.1;
    assert!(cfg.validate().is_err());
}

#[test]
fn quality_preset_parse() {
    assert_eq!(QualityPreset::parse("fast"), Some(QualityPreset::Fast));
    assert_eq!(
        QualityPreset::parse("balanced"),
        Some(QualityPreset::Balanced)
    );
    assert_eq!(QualityPreset::parse("high"), Some(QualityPreset::High));
    assert_eq!(QualityPreset::parse("FAST"), Some(QualityPreset::Fast));
    assert_eq!(QualityPreset::parse("unknown"), None);
}

#[test]
fn quality_preset_display() {
    assert_eq!(format!("{}", QualityPreset::Fast), "fast");
    assert_eq!(format!("{}", QualityPreset::Balanced), "balanced");
    assert_eq!(format!("{}", QualityPreset::High), "high");
}

// ---------------------------------------------------------------------------
// Golden / Snapshot Tests
// ---------------------------------------------------------------------------

#[test]
fn deterministic_output_same_config() {
    // Same input + same config = same SVG output (deterministic)
    let img = ImageBuffer::from_fn(12, 12, |x, y| {
        if x < 6 && y < 6 {
            Rgba([255, 0, 0, 255])
        } else if x >= 6 && y < 6 {
            Rgba([0, 255, 0, 255])
        } else if x < 6 {
            Rgba([0, 0, 255, 255])
        } else {
            Rgba([255, 255, 0, 255])
        }
    });
    let bytes = encode_png(&img);

    let config = QualityPreset::Balanced.to_config();

    let svg1 = Tracer::new(config.clone()).trace_bytes(&bytes).unwrap();
    let svg2 = Tracer::new(config).trace_bytes(&bytes).unwrap();

    assert_eq!(svg1, svg2, "SVG output must be deterministic");
}

#[test]
fn svg_output_structure_golden() {
    // Check that the SVG structure follows expected patterns
    let img = make_solid_image(16, 16, Rgba([100, 200, 50, 255]));
    let bytes = encode_png(&img);

    let tracer = Tracer::with_preset(QualityPreset::Fast);
    let svg = tracer.trace_bytes(&bytes).unwrap();

    // Must have XML declaration
    assert!(svg.starts_with("<?xml"));

    // Must have SVG namespace
    assert!(svg.contains("xmlns=\"http://www.w3.org/2000/svg\""));

    // Must have white background rect
    assert!(svg.contains("<rect"));
    assert!(svg.contains("fill=\"white\""));

    // Must close properly
    assert!(svg.trim_end().ends_with("</svg>"));

    // Should not have any script or style elements (clean output)
    assert!(!svg.contains("<script"));
    assert!(!svg.contains("<style"));
}

#[test]
fn different_presets_produce_different_output() {
    let img = ImageBuffer::from_fn(24, 24, |x, y| {
        let r = ((x as f64 / 24.0) * 255.0) as u8;
        let g = ((y as f64 / 24.0) * 255.0) as u8;
        Rgba([r, g, 128, 255])
    });
    let bytes = encode_png(&img);

    let svg_fast = Tracer::with_preset(QualityPreset::Fast)
        .trace_bytes(&bytes)
        .unwrap();
    let svg_high = Tracer::with_preset(QualityPreset::High)
        .trace_bytes(&bytes)
        .unwrap();

    // High preset should typically produce more detailed output (more paths/longer SVG)
    // At least they should be different due to different settings
    assert_ne!(
        svg_fast, svg_high,
        "Fast and High presets should produce different output for gradient images"
    );
}

// ---------------------------------------------------------------------------
// Error Type Tests
// ---------------------------------------------------------------------------

#[test]
fn error_display_messages() {
    let e = VectizeError::UnsupportedFormat("bmp".to_string());
    let msg = format!("{e}");
    assert!(msg.contains("bmp"));

    let e = VectizeError::InvalidConfig("bad".to_string());
    let msg = format!("{e}");
    assert!(msg.contains("bad"));

    let e = VectizeError::Pipeline("failed".to_string());
    let msg = format!("{e}");
    assert!(msg.contains("failed"));
}

// ---------------------------------------------------------------------------
// Edge Case Tests
// ---------------------------------------------------------------------------

#[test]
fn tiny_image_1x1() {
    let img = make_solid_image(1, 1, Rgba([255, 128, 0, 255]));
    let bytes = encode_png(&img);

    let tracer = Tracer::with_preset(QualityPreset::Fast);
    let svg = tracer.trace_bytes(&bytes).unwrap();
    assert!(svg.contains("<svg"));
    assert!(svg.contains("viewBox=\"0 0 1 1\""));
}

#[test]
fn large_color_count() {
    let img = make_solid_image(8, 8, Rgba([128, 64, 32, 255]));
    let bytes = encode_png(&img);

    let mut config = TracingConfig::default();
    config.color_count = 256; // Maximum
    let tracer = Tracer::new(config);
    let svg = tracer.trace_bytes(&bytes).unwrap();
    assert!(svg.contains("<svg"));
}

#[test]
fn no_smoothing() {
    let img = make_solid_image(16, 16, Rgba([200, 100, 50, 255]));
    let bytes = encode_png(&img);

    let mut config = TracingConfig::default();
    config.smoothing_strength = 0.0;
    let tracer = Tracer::new(config);
    let svg = tracer.trace_bytes(&bytes).unwrap();

    // With zero smoothing, should use linear paths (L commands, no C commands)
    // or at least produce valid SVG
    assert!(svg.contains("<svg"));
}

#[test]
fn denoising_enabled() {
    let img = make_solid_image(16, 16, Rgba([100, 100, 100, 255]));
    let bytes = encode_png(&img);

    let mut config = TracingConfig::default();
    config.enable_denoising = true;
    let tracer = Tracer::new(config);
    let svg = tracer.trace_bytes(&bytes).unwrap();
    assert!(svg.contains("<svg"));
}

#[test]
fn preprocessing_disabled() {
    let img = make_solid_image(16, 16, Rgba([100, 100, 100, 255]));
    let bytes = encode_png(&img);

    let mut config = TracingConfig::default();
    config.enable_preprocessing = false;
    let tracer = Tracer::new(config);
    let svg = tracer.trace_bytes(&bytes).unwrap();
    assert!(svg.contains("<svg"));
}
