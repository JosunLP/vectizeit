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

fn extract_path_numbers(svg: &str) -> Vec<f64> {
    let mut values = Vec::new();
    let mut search_offset = 0;

    while let Some(relative_start) = svg[search_offset..].find(" d=\"") {
        let data_start = search_offset + relative_start + 4;
        let Some(relative_end) = svg[data_start..].find('"') else {
            break;
        };

        values.extend(parse_svg_numbers(
            &svg[data_start..data_start + relative_end],
        ));
        search_offset = data_start + relative_end + 1;
    }

    values
}

fn extract_path_number_tokens(svg: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut search_offset = 0;

    while let Some(relative_start) = svg[search_offset..].find(" d=\"") {
        let data_start = search_offset + relative_start + 4;
        let Some(relative_end) = svg[data_start..].find('"') else {
            break;
        };

        values.extend(parse_svg_number_tokens(
            &svg[data_start..data_start + relative_end],
        ));
        search_offset = data_start + relative_end + 1;
    }

    values
}

fn extract_path_coordinate_pairs(svg: &str) -> Vec<(f64, f64)> {
    let values = extract_path_numbers(svg);
    let pairs: Vec<(f64, f64)> = values
        .chunks_exact(2)
        .map(|chunk| (chunk[0], chunk[1]))
        .collect();

    assert_eq!(
        pairs.len() * 2,
        values.len(),
        "path coordinates must be paired"
    );
    pairs
}

#[derive(Debug, Clone, Copy)]
struct PathBounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

fn path_bounds(points: &[(f64, f64)]) -> PathBounds {
    let mut bounds = PathBounds {
        min_x: f64::INFINITY,
        min_y: f64::INFINITY,
        max_x: f64::NEG_INFINITY,
        max_y: f64::NEG_INFINITY,
    };

    for &(x, y) in points {
        bounds.min_x = bounds.min_x.min(x);
        bounds.min_y = bounds.min_y.min(y);
        bounds.max_x = bounds.max_x.max(x);
        bounds.max_y = bounds.max_y.max(y);
    }

    bounds
}

fn parse_svg_numbers(data: &str) -> Vec<f64> {
    parse_svg_number_tokens(data)
        .into_iter()
        .filter_map(|token| token.parse::<f64>().ok())
        .collect()
}

fn parse_svg_number_tokens(data: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();

    for ch in data.chars() {
        if ch.is_ascii_digit() || ch == '.' || ch == '-' {
            current.push(ch);
            continue;
        }

        if !current.is_empty() {
            values.push(std::mem::take(&mut current));
        }
    }

    if !current.is_empty() {
        values.push(current);
    }

    values
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
fn trace_bytes_emits_smaller_detail_after_larger_background() {
    let img = ImageBuffer::from_fn(12, 12, |x, y| {
        if (4..=7).contains(&x) && (4..=7).contains(&y) {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([0, 0, 255, 255])
        }
    });
    let bytes = encode_png(&img);

    let config = TracingConfig {
        color_count: 8,
        simplification_tolerance: 0.0,
        min_region_area: 0.0,
        smoothing_strength: 0.0,
        corner_sensitivity: 0.6,
        alpha_threshold: 128,
        despeckle_threshold: 0.0,
        enable_denoising: false,
        enable_preprocessing: true,
        quality_preset: QualityPreset::Balanced,
        background_color: None,
    };

    let svg = Tracer::new(config).trace_bytes(&bytes).unwrap();
    let background_index = svg.find("fill=\"#0000ff\"").unwrap();
    let detail_index = svg.find("fill=\"#ff0000\"").unwrap();

    assert!(background_index < detail_index);
}

#[test]
fn trace_bytes_omits_border_connected_white_path() {
    let img = ImageBuffer::from_fn(12, 12, |x, y| {
        if (3..=8).contains(&x) && (3..=8).contains(&y) {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([255, 255, 255, 255])
        }
    });
    let bytes = encode_png(&img);

    let config = TracingConfig {
        color_count: 8,
        simplification_tolerance: 0.0,
        min_region_area: 0.0,
        smoothing_strength: 0.0,
        corner_sensitivity: 0.6,
        alpha_threshold: 128,
        despeckle_threshold: 0.0,
        enable_denoising: false,
        enable_preprocessing: true,
        quality_preset: QualityPreset::Balanced,
        background_color: None,
    };

    let svg = Tracer::new(config).trace_bytes(&bytes).unwrap();

    assert!(svg.contains("fill=\"#ffffff\""));
    assert!(svg.contains("fill=\"#000000\""));
    // The redundant white contour is suppressed, so only 1 path remains.
    assert_eq!(svg.matches("<path").count(), 1);
}

#[test]
fn trace_bytes_keeps_interior_white_island_when_background_is_white() {
    let img = ImageBuffer::from_fn(13, 13, |x, y| {
        if (2..=10).contains(&x)
            && (2..=10).contains(&y)
            && !((5..=7).contains(&x) && (5..=7).contains(&y))
        {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([255, 255, 255, 255])
        }
    });
    let bytes = encode_png(&img);

    let config = TracingConfig {
        color_count: 2,
        simplification_tolerance: 0.0,
        min_region_area: 0.0,
        smoothing_strength: 0.0,
        corner_sensitivity: 0.6,
        alpha_threshold: 128,
        despeckle_threshold: 0.0,
        enable_denoising: false,
        enable_preprocessing: true,
        quality_preset: QualityPreset::Balanced,
        background_color: None,
    };

    let result = Tracer::new(config).trace_bytes_result(&bytes).unwrap();
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");

    assert!(result.svg().contains("fill=\"#ffffff\""));
    assert!(result.svg().contains("M 5.00 5.00"));
    assert_eq!(result.svg().matches("<path").count(), 2);
    assert_eq!(metrics.regions_emitted(), 2);
    assert_eq!(metrics.invalid_contours_discarded(), 0);
    assert_eq!(metrics.contours_simplified_away(), 0);
    assert_eq!(metrics.contours_filtered_min_area(), 0);
    assert_eq!(metrics.contours_suppressed_background(), 1);
}

#[test]
fn trace_bytes_stage_metrics_report_svg_filtered_contours() {
    let img = ImageBuffer::from_fn(12, 12, |x, y| {
        if (5..=6).contains(&x) && (5..=6).contains(&y) {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([255, 255, 255, 255])
        }
    });
    let bytes = encode_png(&img);

    let config = TracingConfig {
        color_count: 2,
        simplification_tolerance: 0.0,
        min_region_area: 10.0,
        smoothing_strength: 0.0,
        corner_sensitivity: 0.6,
        alpha_threshold: 128,
        despeckle_threshold: 0.0,
        enable_denoising: false,
        enable_preprocessing: true,
        quality_preset: QualityPreset::Balanced,
        background_color: None,
    };

    let result = Tracer::new(config).trace_bytes_result(&bytes).unwrap();
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");

    assert_eq!(result.svg().matches("<path").count(), 0);
    assert_eq!(metrics.invalid_contours_discarded(), 0);
    assert_eq!(metrics.contours_simplified_away(), 0);
    assert_eq!(metrics.contours_filtered_min_area(), 1);
    assert_eq!(metrics.contours_suppressed_background(), 1);
    assert_eq!(metrics.contours_emitted(), 0);
    assert_eq!(metrics.regions_emitted(), 0);
}

#[test]
fn trace_bytes_uses_two_decimal_coordinate_precision() {
    let img = make_solid_image(1, 1, Rgba([0, 0, 0, 255]));
    let bytes = encode_png(&img);

    for smoothing_strength in [0.0, 0.6] {
        let config = TracingConfig {
            color_count: 2,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            smoothing_strength,
            corner_sensitivity: 0.6,
            alpha_threshold: 128,
            despeckle_threshold: 0.0,
            enable_denoising: false,
            enable_preprocessing: true,
            quality_preset: QualityPreset::Balanced,
            background_color: None,
        };

        let svg = Tracer::new(config).trace_bytes(&bytes).unwrap();
        let path_tokens = extract_path_number_tokens(&svg);

        assert!(!path_tokens.is_empty());
        assert!(
            path_tokens.iter().all(|token| token
                .split_once('.')
                .is_some_and(|(_, fraction)| fraction.len() == 2)),
            "all path coordinates must use two decimal places for smoothing_strength={smoothing_strength}: {path_tokens:?}"
        );
    }
}

#[test]
fn trace_bytes_smoothing_keeps_edge_touching_coordinates_inside_viewbox() {
    let img = make_solid_image(20, 20, Rgba([0, 0, 0, 255]));
    let bytes = encode_png(&img);

    let config = TracingConfig {
        color_count: 2,
        simplification_tolerance: 0.0,
        min_region_area: 0.0,
        smoothing_strength: 0.8,
        corner_sensitivity: 0.0,
        alpha_threshold: 128,
        despeckle_threshold: 0.0,
        enable_denoising: false,
        enable_preprocessing: true,
        quality_preset: QualityPreset::Balanced,
        background_color: None,
    };

    let result = Tracer::new(config).trace_bytes_result(&bytes).unwrap();
    let path_numbers = extract_path_numbers(result.svg());

    assert!(result.svg().contains(" C "));
    assert!(!path_numbers.is_empty());
    assert!(
        path_numbers
            .iter()
            .all(|value| (0.0..=20.0).contains(value)),
        "edge-touching smoothed coordinates must stay inside the viewBox, got {path_numbers:?}"
    );
}

#[test]
fn trace_bytes_corner_sensitive_smoothing_preserves_more_region_extent() {
    let img = ImageBuffer::from_fn(14, 14, |x, y| {
        if (3..=10).contains(&x) && (3..=10).contains(&y) {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([255, 255, 255, 255])
        }
    });
    let bytes = encode_png(&img);

    let low_corner_config = TracingConfig {
        color_count: 2,
        simplification_tolerance: 0.0,
        min_region_area: 0.0,
        smoothing_strength: 1.0,
        corner_sensitivity: 0.0,
        alpha_threshold: 128,
        despeckle_threshold: 0.0,
        enable_denoising: false,
        enable_preprocessing: true,
        quality_preset: QualityPreset::Balanced,
        background_color: None,
    };
    let high_corner_config = TracingConfig {
        corner_sensitivity: 1.0,
        ..low_corner_config.clone()
    };

    let low_corner_result = Tracer::new(low_corner_config)
        .trace_bytes_result(&bytes)
        .unwrap();
    let high_corner_result = Tracer::new(high_corner_config)
        .trace_bytes_result(&bytes)
        .unwrap();

    let low_bounds = path_bounds(&extract_path_coordinate_pairs(low_corner_result.svg()));
    let high_bounds = path_bounds(&extract_path_coordinate_pairs(high_corner_result.svg()));

    assert!(
        high_bounds.min_x <= low_bounds.min_x,
        "higher corner sensitivity should keep the left edge from drifting inward"
    );
    assert!(
        high_bounds.min_y <= low_bounds.min_y,
        "higher corner sensitivity should keep the top edge from drifting inward"
    );
    assert!(
        high_bounds.max_x >= low_bounds.max_x,
        "higher corner sensitivity should keep the right edge from drifting inward"
    );
    assert!(
        high_bounds.max_y >= low_bounds.max_y,
        "higher corner sensitivity should keep the bottom edge from drifting inward"
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
        background_color: None,
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
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");
    assert!(
        result.debug().regions()[0].hole_count() <= result.debug().regions()[0].contour_count()
    );
    assert_eq!(metrics.invalid_contours_discarded(), 0);
    assert_eq!(metrics.contours_simplified_away(), 0);
    assert_eq!(metrics.contours_filtered_min_area(), 0);
    assert_eq!(metrics.contours_suppressed_background(), 0);
    assert!(metrics.contours_extracted() >= metrics.contours_after_despeckle());
    assert!(metrics.contours_after_despeckle() >= metrics.contours_emitted());
}

#[test]
fn trace_bytes_ring_image_preserves_hole() {
    let img = ImageBuffer::from_fn(9, 9, |x, y| {
        if (1..=7).contains(&x)
            && (1..=7).contains(&y)
            && !((3..=5).contains(&x) && (3..=5).contains(&y))
        {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([255, 255, 255, 255])
        }
    });
    let bytes = encode_png(&img);

    let config = TracingConfig {
        color_count: 2,
        simplification_tolerance: 0.0,
        min_region_area: 0.0,
        smoothing_strength: 0.0,
        corner_sensitivity: 0.6,
        alpha_threshold: 128,
        despeckle_threshold: 0.0,
        enable_denoising: false,
        enable_preprocessing: true,
        quality_preset: QualityPreset::Balanced,
        background_color: None,
    };

    let tracer = Tracer::new(config);
    let result = tracer.trace_bytes_result(&bytes).unwrap();
    let black_region = result
        .debug()
        .regions()
        .iter()
        .find(|region| region.color().to_hex() == "#000000")
        .expect("black ring region should exist");
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");

    assert_eq!(black_region.contour_count(), 2);
    assert_eq!(black_region.hole_count(), 1);
    assert_eq!(metrics.invalid_contours_discarded(), 0);
    assert_eq!(metrics.contours_simplified_away(), 0);
    assert_eq!(metrics.contours_filtered_min_area(), 0);
    assert_eq!(metrics.contours_suppressed_background(), 1);
    assert!(metrics.contours_extracted() >= 2);
    assert!(metrics.holes_extracted() >= 1);
    assert!(metrics.holes_after_despeckle() >= 1);
    assert!(metrics.holes_emitted() >= 1);
    assert!(metrics.regions_emitted() >= 1);
    assert!(result.svg().contains(r#"fill-rule="evenodd""#));
}

#[test]
fn trace_bytes_result_reports_bezier_emitted_points() {
    let img = ImageBuffer::from_fn(24, 24, |x, y| {
        if (4..=19).contains(&x) && (4..=19).contains(&y) {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([255, 255, 255, 255])
        }
    });
    let bytes = encode_png(&img);

    let config = TracingConfig {
        color_count: 2,
        simplification_tolerance: 0.0,
        min_region_area: 0.0,
        smoothing_strength: 0.6,
        corner_sensitivity: 0.6,
        alpha_threshold: 128,
        despeckle_threshold: 0.0,
        enable_denoising: false,
        enable_preprocessing: true,
        quality_preset: QualityPreset::Balanced,
        background_color: None,
    };

    let tracer = Tracer::new(config);
    let result = tracer.trace_bytes_result(&bytes).unwrap();
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");
    let expected_emitted_points = result.svg().matches("M ").count()
        + result.svg().matches(" L ").count()
        + (result.svg().matches(" C ").count() * 3);

    assert!(result.svg().contains(" C "));
    assert!(metrics.contours_emitted() >= 1);
    assert_eq!(metrics.points_emitted(), expected_emitted_points);
}

#[test]
fn trace_bytes_result_uses_closed_beziers_for_closed_contours() {
    let img = make_solid_image(20, 20, Rgba([0, 0, 0, 255]));
    let bytes = encode_png(&img);

    let config = TracingConfig {
        color_count: 2,
        simplification_tolerance: 0.0,
        min_region_area: 0.0,
        smoothing_strength: 0.6,
        corner_sensitivity: 0.6,
        alpha_threshold: 128,
        despeckle_threshold: 0.0,
        enable_denoising: false,
        enable_preprocessing: true,
        quality_preset: QualityPreset::Balanced,
        background_color: None,
    };

    let tracer = Tracer::new(config);
    let result = tracer.trace_bytes_result(&bytes).unwrap();
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");
    let path_numbers = extract_path_numbers(result.svg());

    assert_eq!(result.svg().matches(" C ").count(), 4);
    assert_eq!(metrics.contours_emitted(), 1);
    assert_eq!(metrics.points_emitted(), 13);
    assert_eq!(metrics.invalid_contours_discarded(), 0);
    assert_eq!(metrics.contours_simplified_away(), 0);
    assert_eq!(metrics.contours_filtered_min_area(), 0);
    assert_eq!(metrics.contours_suppressed_background(), 0);
    assert!(!path_numbers.is_empty());
    assert!(
        path_numbers
            .iter()
            .all(|value| (0.0..=20.0).contains(value)),
        "smoothed coordinates must stay inside the viewBox, got {path_numbers:?}"
    );
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

    assert!(output_path.exists());
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
    let cfg = TracingConfig {
        color_count: 0,
        ..TracingConfig::default()
    };
    assert!(cfg.validate().is_err());

    let cfg = TracingConfig {
        color_count: 1,
        ..TracingConfig::default()
    };
    assert!(cfg.validate().is_err());

    let cfg = TracingConfig {
        color_count: 256,
        ..TracingConfig::default()
    };
    assert!(cfg.validate().is_ok());

    let cfg = TracingConfig {
        color_count: 257,
        ..TracingConfig::default()
    };
    assert!(cfg.validate().is_err());

    // Invalid: negative tolerance
    let cfg = TracingConfig {
        simplification_tolerance: -0.1,
        ..TracingConfig::default()
    };
    assert!(cfg.validate().is_err());

    // Invalid: negative min_region_area
    let cfg = TracingConfig {
        min_region_area: -1.0,
        ..TracingConfig::default()
    };
    assert!(cfg.validate().is_err());

    // Invalid: smoothing out of range
    let cfg = TracingConfig {
        smoothing_strength: 1.5,
        ..TracingConfig::default()
    };
    assert!(cfg.validate().is_err());

    let cfg = TracingConfig {
        smoothing_strength: -0.1,
        ..TracingConfig::default()
    };
    assert!(cfg.validate().is_err());

    // Invalid: corner sensitivity out of range
    let cfg = TracingConfig {
        corner_sensitivity: 2.0,
        ..TracingConfig::default()
    };
    assert!(cfg.validate().is_err());

    let cfg = TracingConfig {
        corner_sensitivity: -0.1,
        ..TracingConfig::default()
    };
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
    assert!(svg.contains("fill=\"#ffffff\""));

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

#[test]
fn high_preset_preserves_more_color_detail_than_balanced_on_gradients() {
    let img = ImageBuffer::from_fn(32, 32, |x, y| {
        let r = ((x as f64 / 31.0) * 255.0) as u8;
        let g = ((y as f64 / 31.0) * 255.0) as u8;
        let b = (((x + y) as f64 / 62.0) * 255.0) as u8;
        Rgba([r, g, b, 255])
    });
    let bytes = encode_png(&img);

    let balanced = Tracer::with_preset(QualityPreset::Balanced)
        .trace_bytes_result(&bytes)
        .unwrap();
    let high = Tracer::with_preset(QualityPreset::High)
        .trace_bytes_result(&bytes)
        .unwrap();

    assert!(
        high.debug().palette().len() > balanced.debug().palette().len(),
        "high preset should retain a richer palette for detailed gradients"
    );
    assert!(
        high.svg().len() > balanced.svg().len(),
        "high preset should emit more geometric detail for detailed gradients"
    );
}

#[test]
fn trace_bytes_result_preserves_small_accent_palette_color() {
    let img = ImageBuffer::from_fn(12, 12, |_, y| {
        if y == 0 {
            Rgba([224, 16, 16, 255])
        } else {
            Rgba([24, 16, 16, 255])
        }
    });
    let bytes = encode_png(&img);

    let config = TracingConfig {
        color_count: 2,
        simplification_tolerance: 0.0,
        min_region_area: 0.0,
        smoothing_strength: 0.0,
        corner_sensitivity: 0.6,
        alpha_threshold: 128,
        despeckle_threshold: 0.0,
        enable_denoising: false,
        enable_preprocessing: true,
        quality_preset: QualityPreset::Balanced,
        background_color: None,
    };

    let result = Tracer::new(config).trace_bytes_result(&bytes).unwrap();

    assert!(
        result.debug().palette().iter().any(|color| color.r <= 40),
        "the majority base tone should remain in the quantized palette"
    );
    assert!(
        result.debug().palette().iter().any(|color| color.r >= 200),
        "the small accent tone should remain in the quantized palette"
    );
}

#[test]
fn trace_bytes_result_collapses_antialias_bridge_palette_color_for_flat_art() {
    let bridge = Rgba([128, 64, 0, 255]);
    let img = ImageBuffer::from_fn(16, 16, |x, y| {
        let diagonal = x + y;

        if diagonal < 14 {
            Rgba([0, 0, 0, 255])
        } else if diagonal == 14 {
            bridge
        } else {
            Rgba([255, 128, 0, 255])
        }
    });
    let bytes = encode_png(&img);

    let config = TracingConfig {
        color_count: 3,
        simplification_tolerance: 0.0,
        min_region_area: 0.0,
        smoothing_strength: 0.0,
        corner_sensitivity: 0.9,
        alpha_threshold: 128,
        despeckle_threshold: 0.0,
        enable_denoising: false,
        enable_preprocessing: false,
        quality_preset: QualityPreset::Balanced,
        background_color: None,
    };

    let result = Tracer::new(config).trace_bytes_result(&bytes).unwrap();

    assert_eq!(
        result.debug().palette().len(),
        2,
        "flat-art antialias bridge shades should collapse into the endpoint palette"
    );
    assert_eq!(
        result.svg().matches("<path").count(),
        2,
        "collapsing bridge shades should prevent extra contour bands"
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

    let e = VectizeError::OutputExists("out.svg".to_string());
    let msg = format!("{e}");
    assert!(msg.contains("out.svg"));
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

    let config = TracingConfig {
        color_count: 256, // maximum
        ..TracingConfig::default()
    };
    let tracer = Tracer::new(config);
    let svg = tracer.trace_bytes(&bytes).unwrap();
    assert!(svg.contains("<svg"));
}

#[test]
fn no_smoothing() {
    let img = make_solid_image(16, 16, Rgba([200, 100, 50, 255]));
    let bytes = encode_png(&img);

    let config = TracingConfig {
        smoothing_strength: 0.0,
        ..TracingConfig::default()
    };
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

    let config = TracingConfig {
        enable_denoising: true,
        ..TracingConfig::default()
    };
    let tracer = Tracer::new(config);
    let svg = tracer.trace_bytes(&bytes).unwrap();
    assert!(svg.contains("<svg"));
}

#[test]
fn preprocessing_disabled() {
    let img = make_solid_image(16, 16, Rgba([100, 100, 100, 255]));
    let bytes = encode_png(&img);

    let config = TracingConfig {
        enable_preprocessing: false,
        ..TracingConfig::default()
    };
    let tracer = Tracer::new(config);
    let svg = tracer.trace_bytes(&bytes).unwrap();
    assert!(svg.contains("<svg"));
}
