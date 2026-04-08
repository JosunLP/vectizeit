//! CLI smoke tests for the `trace` binary.
//!
//! These tests verify that the CLI binary starts, parses arguments correctly,
//! and produces expected outputs for basic operations.

use std::process::Command;

/// Helper: get the path to the `trace` binary.
fn trace_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_trace"))
}

fn create_test_bitmap(name: &str, format: image::ImageFormat) -> std::path::PathBuf {
    use image::{ImageBuffer, Rgba, RgbaImage};

    let path = std::env::temp_dir().join(name);
    let img: RgbaImage = ImageBuffer::from_fn(16, 16, |x, y| {
        if x < 8 {
            Rgba([255, 0, 0, 255])
        } else if y < 8 {
            Rgba([0, 255, 0, 255])
        } else {
            Rgba([0, 0, 255, 255])
        }
    });
    img.save_with_format(&path, format).unwrap();
    path
}

/// Helper: create a temporary test PNG file and return its path.
fn create_test_png(name: &str) -> std::path::PathBuf {
    create_test_bitmap(name, image::ImageFormat::Png)
}

// ---------------------------------------------------------------------------
// Help & Version
// ---------------------------------------------------------------------------

#[test]
fn cli_help_flag() {
    let output = trace_bin().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("trace"));
    assert!(stdout.contains("convert"));
    assert!(stdout.contains("batch"));
}

#[test]
fn cli_version_flag() {
    let output = trace_bin().arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain a version number
    assert!(stdout.contains("0.1.0") || stdout.contains("trace"));
}

#[test]
fn cli_convert_help() {
    let output = trace_bin().args(["convert", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Input bitmap image file"));
    assert!(stdout.contains("--preset"));
    assert!(stdout.contains("--output"));
    assert!(stdout.contains("--colors"));
    assert!(stdout.contains("--corner-sensitivity"));
    assert!(stdout.contains("--despeckle-threshold"));
    assert!(stdout.contains("--background-color"));
    assert!(stdout.contains("--gradients"));
    assert!(stdout.contains("--tile-size"));
    assert!(stdout.contains("--stdout"));
}

#[test]
fn cli_batch_help() {
    let output = trace_bin().args(["batch", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Input directory containing bitmap image files"));
    assert!(stdout.contains("Output directory"));
    assert!(stdout.contains("--format"));
    assert!(stdout.contains("--preset"));
    assert!(stdout.contains("--background-color"));
    assert!(stdout.contains("--overwrite"));
}

// ---------------------------------------------------------------------------
// Convert Subcommand
// ---------------------------------------------------------------------------

#[test]
fn cli_convert_to_stdout() {
    for path in [
        create_test_png("cli_test_stdout.png"),
        create_test_bitmap("cli_test_stdout.bmp", image::ImageFormat::Bmp),
    ] {
        let output = trace_bin()
            .args(["convert", path.to_str().unwrap(), "--stdout"])
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("<svg"), "Expected SVG output on stdout");
        assert!(stdout.contains("</svg>"));

        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn cli_convert_to_file() {
    let png = create_test_png("cli_test_file.png");
    let svg_path = std::env::temp_dir().join("cli_test_file.svg");

    // Remove any previous output
    let _ = std::fs::remove_file(&svg_path);

    let output = trace_bin()
        .args([
            "convert",
            png.to_str().unwrap(),
            "-o",
            svg_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(svg_path.exists(), "SVG output file should be created");

    let svg = std::fs::read_to_string(&svg_path).unwrap();
    assert!(svg.contains("<svg"));

    let _ = std::fs::remove_file(&png);
    let _ = std::fs::remove_file(&svg_path);
}

#[test]
fn cli_convert_with_preset() {
    let png = create_test_png("cli_test_preset.png");

    for preset in ["fast", "balanced", "high"] {
        let output = trace_bin()
            .args([
                "convert",
                png.to_str().unwrap(),
                "--stdout",
                "--preset",
                preset,
            ])
            .output()
            .unwrap();

        assert!(
            output.status.success(),
            "Preset '{preset}' failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("<svg"),
            "Preset '{preset}' did not produce SVG"
        );
    }

    let _ = std::fs::remove_file(&png);
}

#[test]
fn cli_convert_with_options() {
    let png = create_test_png("cli_test_options.png");

    let output = trace_bin()
        .args([
            "convert",
            png.to_str().unwrap(),
            "--stdout",
            "--colors",
            "8",
            "--tolerance",
            "0.5",
            "--smoothing",
            "0.3",
            "--corner-sensitivity",
            "0.7",
            "--min-area",
            "2.0",
            "--alpha-threshold",
            "100",
            "--despeckle-threshold",
            "1.5",
            "--gradients",
            "--tile-size",
            "8",
            "--no-preprocess",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("<svg"));

    let _ = std::fs::remove_file(&png);
}

#[test]
fn cli_convert_with_background_color() {
    let png = create_test_png("cli_test_background_color.png");

    let output = trace_bin()
        .args([
            "convert",
            png.to_str().unwrap(),
            "--stdout",
            "--background-color",
            "102030",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(r##"fill="#102030""##));

    let _ = std::fs::remove_file(&png);
}

#[test]
fn cli_convert_denoise_flag() {
    let png = create_test_png("cli_test_denoise.png");

    let output = trace_bin()
        .args(["convert", png.to_str().unwrap(), "--stdout", "--denoise"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("<svg"));

    let _ = std::fs::remove_file(&png);
}

#[test]
fn cli_convert_verbose_flag() {
    let png = create_test_png("cli_test_verbose.png");

    let output = trace_bin()
        .args(["convert", png.to_str().unwrap(), "--stdout", "--verbose"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Verbose mode should produce log output on stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Tracing") || stderr.contains("Pipeline") || stderr.contains("DEBUG"),
        "Verbose mode should produce some log output, got: {stderr}"
    );

    let _ = std::fs::remove_file(&png);
}

// ---------------------------------------------------------------------------
// Error Cases
// ---------------------------------------------------------------------------

#[test]
fn cli_convert_nonexistent_file() {
    let output = trace_bin()
        .args(["convert", "/nonexistent/file.png", "--stdout"])
        .output()
        .unwrap();

    assert!(!output.status.success());
}

#[test]
fn cli_convert_invalid_preset() {
    let png = create_test_png("cli_test_invalid_preset.png");

    let output = trace_bin()
        .args([
            "convert",
            png.to_str().unwrap(),
            "--stdout",
            "--preset",
            "ultra",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());

    let _ = std::fs::remove_file(&png);
}

#[test]
fn cli_convert_invalid_background_color() {
    let png = create_test_png("cli_test_invalid_background_color.png");

    let output = trace_bin()
        .args([
            "convert",
            png.to_str().unwrap(),
            "--stdout",
            "--background-color",
            "not-a-color",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());

    let _ = std::fs::remove_file(&png);
}

#[test]
fn cli_no_overwrite_without_flag() {
    let png = create_test_png("cli_test_no_overwrite.png");
    let svg_path = std::env::temp_dir().join("cli_test_no_overwrite.svg");

    // Create existing output file
    std::fs::write(&svg_path, "existing content").unwrap();

    let output = trace_bin()
        .args([
            "convert",
            png.to_str().unwrap(),
            "-o",
            svg_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();

    // Should fail because file exists and --overwrite is not set
    assert!(!output.status.success());

    // Original content should be preserved
    let content = std::fs::read_to_string(&svg_path).unwrap();
    assert_eq!(content, "existing content");

    let _ = std::fs::remove_file(&png);
    let _ = std::fs::remove_file(&svg_path);
}

#[test]
fn cli_overwrite_with_flag() {
    let png = create_test_png("cli_test_overwrite.png");
    let svg_path = std::env::temp_dir().join("cli_test_overwrite.svg");

    // Create existing output file
    std::fs::write(&svg_path, "old content").unwrap();

    let output = trace_bin()
        .args([
            "convert",
            png.to_str().unwrap(),
            "-o",
            svg_path.to_str().unwrap(),
            "--overwrite",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Content should be the new SVG
    let content = std::fs::read_to_string(&svg_path).unwrap();
    assert!(
        content.contains("<svg"),
        "File should contain new SVG content"
    );

    let _ = std::fs::remove_file(&png);
    let _ = std::fs::remove_file(&svg_path);
}

// ---------------------------------------------------------------------------
// Batch Subcommand
// ---------------------------------------------------------------------------

#[test]
fn cli_batch_converts_directory() {
    let input_dir = std::env::temp_dir().join("cli_test_batch_input");
    let output_dir = std::env::temp_dir().join("cli_test_batch_output");

    // Clean up from any previous run
    let _ = std::fs::remove_dir_all(&input_dir);
    let _ = std::fs::remove_dir_all(&output_dir);

    std::fs::create_dir_all(&input_dir).unwrap();

    // Create test images in input directory using two supported bitmap formats.
    let img: image::RgbaImage =
        image::ImageBuffer::from_fn(8, 8, |_, _| image::Rgba([128, 64, 32, 255]));
    img.save(input_dir.join("a.png")).unwrap();
    img.save_with_format(input_dir.join("b.bmp"), image::ImageFormat::Bmp)
        .unwrap();

    let output = trace_bin()
        .args([
            "batch",
            input_dir.to_str().unwrap(),
            output_dir.to_str().unwrap(),
            "--format",
            "svg",
            "--preset",
            "fast",
            "--tolerance",
            "0.8",
            "--min-area",
            "2.0",
            "--smoothing",
            "0.2",
            "--corner-sensitivity",
            "0.9",
            "--alpha-threshold",
            "100",
            "--despeckle-threshold",
            "1.0",
            "--denoise",
            "--no-preprocess",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Check that output SVG files were created
    assert!(output_dir.join("a.svg").exists(), "a.svg should exist");
    assert!(output_dir.join("b.svg").exists(), "b.svg should exist");

    let svg = std::fs::read_to_string(output_dir.join("a.svg")).unwrap();
    assert!(svg.contains("<svg"));

    let _ = std::fs::remove_dir_all(&input_dir);
    let _ = std::fs::remove_dir_all(&output_dir);
}

#[test]
fn cli_batch_nonexistent_input_dir() {
    let output = trace_bin()
        .args(["batch", "/nonexistent/input", "/tmp/output"])
        .output()
        .unwrap();

    assert!(!output.status.success());
}

#[test]
fn cli_no_args_shows_help() {
    let output = trace_bin().output().unwrap();
    // Should fail with usage info when no subcommand given
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Usage") || stderr.contains("trace"),
        "Should show usage info, got: {stderr}"
    );
}
