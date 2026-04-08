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
use std::sync::atomic::{AtomicUsize, Ordering};

use clap::{Args, Parser, Subcommand, ValueEnum};
use log::{debug, error, info, warn};
use rayon::prelude::*;
use vectize::{
    is_supported_bitmap_path, QualityPreset, Tracer, TracingConfig, TracingConfigOverrides,
};

/// trace: high-quality raster-to-vector image tracing tool
#[derive(Parser, Debug)]
#[command(
    name = "trace",
    version,
    about = "Convert bitmap images (PNG, JPEG/JPG, WebP, BMP, GIF, TIFF, TGA, ICO, PNM) to SVG vector graphics",
    long_about = "trace converts bitmap images into clean SVG vector graphics using a \
        multi-stage processing pipeline. It supports common bitmap inputs such as \
        PNG, JPEG/JPG, WebP, BMP, GIF, TIFF, TGA, ICO, and PNM.\n\n\
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
    /// Convert a single bitmap image file to SVG
    #[command(name = "convert", alias = "c")]
    Convert(ConvertArgs),

    /// Batch convert all supported bitmap images in a directory
    #[command(name = "batch", alias = "b")]
    Batch(BatchArgs),
}

/// Arguments for single-file conversion.
#[derive(Args, Debug)]
struct ConvertArgs {
    /// Input bitmap image file
    input: PathBuf,

    /// Output SVG file (defaults to input filename with .svg extension)
    #[arg(short, long)]
    output: Option<PathBuf>,

    #[command(flatten)]
    trace: TraceConfigArgs,

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
    /// Input directory containing bitmap image files
    input_dir: PathBuf,

    /// Output directory for SVG files
    output_dir: PathBuf,

    /// Output format (currently only SVG is supported)
    #[arg(long, value_enum, default_value_t = OutputFormat::Svg)]
    format: OutputFormat,

    #[command(flatten)]
    trace: TraceConfigArgs,

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

/// Shared tracing configuration arguments for both single-file and batch conversion.
#[derive(Args, Debug, Clone)]
struct TraceConfigArgs {
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

    /// Corner sensitivity 0.0–1.0 (higher preserves sharper corners during smoothing and curve fitting)
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

    /// Background color as hex (e.g. "#ff0000" or "00ff00"); default is white
    #[arg(long)]
    background_color: Option<String>,

    /// Approximate smooth region color variation with SVG gradients
    #[arg(long)]
    gradients: bool,

    /// Process palette assignment in tiles to reduce peak segmentation memory
    #[arg(long)]
    tile_size: Option<u32>,
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

fn build_config(args: &TraceConfigArgs) -> Result<TracingConfig, String> {
    let preset = QualityPreset::parse(&args.preset).ok_or_else(|| {
        format!(
            "Unknown preset '{}'. Use: fast, balanced, high",
            args.preset
        )
    })?;
    let overrides = trace_config_overrides(args)?;
    TracingConfig::from_preset_with_overrides(preset, &overrides)
}

fn trace_config_overrides(args: &TraceConfigArgs) -> Result<TracingConfigOverrides, String> {
    Ok(TracingConfigOverrides {
        color_count: args.colors,
        simplification_tolerance: args.tolerance,
        min_region_area: args.min_area,
        smoothing_strength: args.smoothing,
        corner_sensitivity: args.corner_sensitivity,
        alpha_threshold: args.alpha_threshold,
        despeckle_threshold: args.despeckle_threshold,
        enable_denoising: args.denoise.then_some(true),
        enable_preprocessing: args.no_preprocess.then_some(false),
        background_color: args
            .background_color
            .as_deref()
            .map(TracingConfig::parse_hex_color)
            .transpose()?,
        enable_svg_gradients: args.gradients.then_some(true),
        tile_size: args.tile_size,
    })
}

fn run_convert(args: &ConvertArgs) -> Result<(), Box<dyn std::error::Error>> {
    let config = build_config(&args.trace).map_err(vectize::VectizeError::InvalidConfig)?;

    info!(
        "Tracing '{}' with preset '{}'",
        args.input.display(),
        args.trace.preset
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
    let config = build_config(&args.trace).map_err(vectize::VectizeError::InvalidConfig)?;

    if !args.input_dir.is_dir() {
        return Err(Box::new(vectize::VectizeError::InvalidConfig(format!(
            "'{}' is not a directory",
            args.input_dir.display()
        ))));
    }

    std::fs::create_dir_all(&args.output_dir)?;

    let tracer = Tracer::new(config);
    let total = AtomicUsize::new(0);
    let succeeded = AtomicUsize::new(0);

    let entries: Vec<_> = std::fs::read_dir(&args.input_dir)?
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            let path = entry.path();
            path.is_file() && is_supported_bitmap_path(&path)
        })
        .collect();

    entries.par_iter().for_each(|entry| {
        let path = entry.path();
        total.fetch_add(1, Ordering::Relaxed);

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
            return;
        }

        info!("Tracing '{}'", path.display());
        match tracer.trace_file_result(&path) {
            Ok(result) => {
                log_stage_metrics(&result);
                if let Err(e) = result.write_svg(&output_path, args.overwrite) {
                    error!("Failed to write '{}': {e}", output_path.display());
                } else {
                    info!("  → '{}'", output_path.display());
                    succeeded.fetch_add(1, Ordering::Relaxed);
                }
            }
            Err(e) => {
                error!("Failed to trace '{}': {e}", path.display());
            }
        }
    });

    let total = total.load(Ordering::Relaxed);
    let succeeded = succeeded.load(Ordering::Relaxed);
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
