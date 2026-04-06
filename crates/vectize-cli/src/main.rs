//! # trace
//!
//! Command-line interface for the vectize raster-to-vector tracing tool.
//!
//! ## Usage
//!
//! ```text
//! trace convert input.png -o output.svg
//! trace convert input.webp --preset high
//! trace batch ./input-dir ./output-dir --format svg
//! ```

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use log::{debug, error, info, warn};
use vectize::{QualityPreset, Tracer, TracingConfig};

/// trace: high-quality raster-to-vector image tracing tool
#[derive(Parser, Debug)]
#[command(
    name = "trace",
    version,
    about = "Convert raster images (PNG, JPEG, WebP) to SVG vector graphics",
    long_about = "trace converts bitmap images into clean SVG vector graphics using a \
        multi-stage processing pipeline. It supports PNG, JPEG, and WebP inputs.\n\n\
        Examples:\n  \
        trace convert input.png -o output.svg\n  \
        trace convert input.webp --preset high\n  \
        trace batch ./input-dir ./output-dir --format svg"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Convert a single image file to SVG
    #[command(name = "convert", alias = "c")]
    Convert(ConvertArgs),

    /// Batch convert all images in a directory
    #[command(name = "batch", alias = "b")]
    Batch(BatchArgs),
}

/// Arguments for single-file conversion.
#[derive(Args, Debug)]
struct ConvertArgs {
    /// Input image file (PNG, JPEG, or WebP)
    input: PathBuf,

    /// Output SVG file (defaults to input filename with .svg extension)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Quality preset: fast, balanced (default), or high
    #[arg(long, default_value = "balanced")]
    preset: String,

    /// Number of colors in the output palette (2–256)
    #[arg(long)]
    colors: Option<u16>,

    /// Polygon simplification tolerance in pixels (default: 1.0)
    #[arg(long)]
    tolerance: Option<f64>,

    /// Minimum region area in pixels to include (default: 4.0)
    #[arg(long)]
    min_area: Option<f64>,

    /// Smoothing strength 0.0–1.0 (default: 0.5)
    #[arg(long)]
    smoothing: Option<f64>,

    /// Corner sensitivity 0.0–1.0 (higher preserves sharper corners)
    #[arg(long)]
    corner_sensitivity: Option<f64>,

    /// Enable Gaussian denoising before tracing
    #[arg(long)]
    denoise: bool,

    /// Disable preprocessing before tracing
    #[arg(long)]
    no_preprocess: bool,

    /// Alpha threshold 0–255; pixels below are transparent (default: 128)
    #[arg(long)]
    alpha_threshold: Option<u8>,

    /// Minimum contour perimeter to keep during despeckling
    #[arg(long)]
    despeckle_threshold: Option<f64>,

    /// Overwrite output file if it already exists
    #[arg(long)]
    overwrite: bool,

    /// Write SVG to stdout instead of a file
    #[arg(long)]
    stdout: bool,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

/// Arguments for batch conversion.
#[derive(Args, Debug)]
struct BatchArgs {
    /// Input directory containing image files
    input_dir: PathBuf,

    /// Output directory for SVG files
    output_dir: PathBuf,

    /// Output format (currently only SVG is supported)
    #[arg(long, value_enum, default_value_t = OutputFormat::Svg)]
    format: OutputFormat,

    /// Quality preset: fast, balanced (default), or high
    #[arg(long, default_value = "balanced")]
    preset: String,

    /// Number of colors in the output palette (2–256)
    #[arg(long)]
    colors: Option<u16>,

    /// Polygon simplification tolerance in pixels
    #[arg(long)]
    tolerance: Option<f64>,

    /// Minimum region area in pixels to include
    #[arg(long)]
    min_area: Option<f64>,

    /// Smoothing strength 0.0–1.0
    #[arg(long)]
    smoothing: Option<f64>,

    /// Corner sensitivity 0.0–1.0 (higher preserves sharper corners)
    #[arg(long)]
    corner_sensitivity: Option<f64>,

    /// Alpha threshold 0–255; pixels below are transparent
    #[arg(long)]
    alpha_threshold: Option<u8>,

    /// Minimum contour perimeter to keep during despeckling
    #[arg(long)]
    despeckle_threshold: Option<f64>,

    /// Enable Gaussian denoising before tracing
    #[arg(long)]
    denoise: bool,

    /// Disable preprocessing before tracing
    #[arg(long)]
    no_preprocess: bool,

    /// Overwrite existing output files
    #[arg(long)]
    overwrite: bool,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Svg,
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Command::Convert(args) => {
            init_logger(args.verbose);
            if let Err(e) = run_convert(args) {
                error!("Conversion failed: {e}");
                std::process::exit(1);
            }
        }
        Command::Batch(args) => {
            init_logger(args.verbose);
            if let Err(e) = run_batch(args) {
                error!("Batch conversion failed: {e}");
                std::process::exit(1);
            }
        }
    }
}

fn init_logger(verbose: bool) {
    let level = if verbose { "debug" } else { "info" };
    env_logger::Builder::new()
        .parse_filters(level)
        .format_timestamp(None)
        .init();
}

fn build_config(preset_str: &str, overrides: ConfigOverrides) -> Result<TracingConfig, String> {
    let preset = QualityPreset::parse(preset_str)
        .ok_or_else(|| format!("Unknown preset '{preset_str}'. Use: fast, balanced, high"))?;

    let mut config = preset.to_config();

    if let Some(c) = overrides.colors {
        config.color_count = c;
    }
    if let Some(t) = overrides.tolerance {
        config.simplification_tolerance = t;
    }
    if let Some(a) = overrides.min_area {
        config.min_region_area = a;
    }
    if let Some(s) = overrides.smoothing {
        config.smoothing_strength = s;
    }
    if let Some(cs) = overrides.corner_sensitivity {
        config.corner_sensitivity = cs;
    }
    if let Some(at) = overrides.alpha_threshold {
        config.alpha_threshold = at;
    }
    if let Some(dt) = overrides.despeckle_threshold {
        config.despeckle_threshold = dt;
    }
    if overrides.denoise {
        config.enable_denoising = true;
    }
    if overrides.no_preprocess {
        config.enable_preprocessing = false;
    }

    config.validate()?;
    Ok(config)
}

struct ConfigOverrides {
    colors: Option<u16>,
    tolerance: Option<f64>,
    min_area: Option<f64>,
    smoothing: Option<f64>,
    corner_sensitivity: Option<f64>,
    alpha_threshold: Option<u8>,
    despeckle_threshold: Option<f64>,
    denoise: bool,
    no_preprocess: bool,
}

fn run_convert(args: &ConvertArgs) -> Result<(), Box<dyn std::error::Error>> {
    let config = build_config(
        &args.preset,
        ConfigOverrides {
            colors: args.colors,
            tolerance: args.tolerance,
            min_area: args.min_area,
            smoothing: args.smoothing,
            corner_sensitivity: args.corner_sensitivity,
            alpha_threshold: args.alpha_threshold,
            despeckle_threshold: args.despeckle_threshold,
            denoise: args.denoise,
            no_preprocess: args.no_preprocess,
        },
    )
    .map_err(vectize::VectizeError::InvalidConfig)?;

    info!(
        "Tracing '{}' with preset '{}'",
        args.input.display(),
        args.preset
    );

    let tracer = Tracer::new(config);
    let result = tracer.trace_file_result(&args.input)?;
    log_stage_metrics(&result);

    if args.stdout {
        print!("{}", result.svg());
        return Ok(());
    }

    let output_path = args
        .output
        .clone()
        .unwrap_or_else(|| args.input.with_extension("svg"));

    if output_path.exists() && !args.overwrite {
        warn!(
            "Output file '{}' already exists. Use --overwrite to replace it.",
            output_path.display()
        );
    }

    result.write_svg(&output_path, args.overwrite)?;
    info!("Saved SVG to '{}'", output_path.display());

    Ok(())
}

fn run_batch(args: &BatchArgs) -> Result<(), Box<dyn std::error::Error>> {
    let config = build_config(
        &args.preset,
        ConfigOverrides {
            colors: args.colors,
            tolerance: args.tolerance,
            min_area: args.min_area,
            smoothing: args.smoothing,
            corner_sensitivity: args.corner_sensitivity,
            alpha_threshold: args.alpha_threshold,
            despeckle_threshold: args.despeckle_threshold,
            denoise: args.denoise,
            no_preprocess: args.no_preprocess,
        },
    )
    .map_err(vectize::VectizeError::InvalidConfig)?;

    if !args.input_dir.is_dir() {
        return Err(Box::new(vectize::VectizeError::InvalidConfig(format!(
            "'{}' is not a directory",
            args.input_dir.display()
        ))));
    }

    std::fs::create_dir_all(&args.output_dir)?;

    let extensions = ["png", "jpg", "jpeg", "webp"];
    let tracer = Tracer::new(config);
    let mut total = 0usize;
    let mut succeeded = 0usize;

    for entry in std::fs::read_dir(&args.input_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if !extensions.contains(&ext.as_str()) {
            continue;
        }

        total += 1;
        let stem = path.file_stem().unwrap_or_default();
        let output_path = args
            .output_dir
            .join(stem)
            .with_extension(args.format.extension());

        if output_path.exists() && !args.overwrite {
            warn!(
                "Skipping '{}': output already exists (use --overwrite)",
                path.display()
            );
            continue;
        }

        info!("Tracing '{}'", path.display());
        match tracer.trace_file_result(&path) {
            Ok(result) => {
                log_stage_metrics(&result);
                if let Err(e) = result.write_svg(&output_path, args.overwrite) {
                    error!("Failed to write '{}': {e}", output_path.display());
                } else {
                    info!("  → '{}'", output_path.display());
                    succeeded += 1;
                }
            }
            Err(e) => {
                error!("Failed to trace '{}': {e}", path.display());
            }
        }
    }

    info!("Batch complete: {succeeded}/{total} files converted successfully.");
    Ok(())
}

fn log_stage_metrics(result: &vectize::TracingResult) {
    let Some(metrics) = result.stage_metrics() else {
        return;
    };

    if metrics.invalid_contours_discarded() > 0 {
        warn!(
            "Dropped {} invalid contours during extraction; the output may omit malformed regions.",
            metrics.invalid_contours_discarded()
        );
    }

    debug!(
        "Trace metrics: extracted_contours={} extracted_holes={} extracted_points={} invalid_contours_discarded={} after_despeckle={} svg_simplified_away={} svg_filtered_min_area={} svg_suppressed_background={} emitted_regions={} emitted_contours={} emitted_holes={} emitted_points={}",
        metrics.contours_extracted(),
        metrics.holes_extracted(),
        metrics.points_extracted(),
        metrics.invalid_contours_discarded(),
        metrics.contours_after_despeckle(),
        metrics.contours_simplified_away(),
        metrics.contours_filtered_min_area(),
        metrics.contours_suppressed_background(),
        metrics.regions_emitted(),
        metrics.contours_emitted(),
        metrics.holes_emitted(),
        metrics.points_emitted()
    );
}

impl OutputFormat {
    fn extension(self) -> &'static str {
        match self {
            OutputFormat::Svg => "svg",
        }
    }
}
