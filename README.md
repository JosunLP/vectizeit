# vectizeit

High-quality raster-to-vector image tracing tool and library written in Rust.

`vectizeit` converts bitmap images (PNG, JPEG, WebP) into clean SVG vector graphics through a
multi-stage processing pipeline — from color quantization and contour tracing to Bezier curve
fitting and SVG emission.

---

## Repository Layout

```bash
vectizeit/
├── Cargo.toml                  # Workspace manifest
├── crates/
│   ├── vectize/                # Core library crate
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── error.rs
│   │   │   ├── config.rs
│   │   │   ├── result.rs
│   │   │   └── pipeline/
│   │   │       ├── mod.rs
│   │   │       ├── loader.rs
│   │   │       ├── preprocess.rs
│   │   │       ├── segment.rs
│   │   │       ├── contour.rs
│   │   │       ├── simplify.rs
│   │   │       ├── smooth.rs
│   │   │       ├── curves.rs
│   │   │       └── svg.rs
│   │   └── tests/
│   │       └── integration_tests.rs  # API & golden tests (44 tests)
│   └── vectize-cli/            # CLI binary crate (`trace`)
│       ├── src/main.rs
│       └── tests/
│           └── cli_smoke_tests.rs    # CLI smoke tests (19 tests)
│   └── vectize-wasm/           # `wasm-bindgen` bindings for JS/TS consumers
│       ├── Cargo.toml
│       └── src/lib.rs
```

---

## Build Instructions

Requires Rust 1.70 or later (stable). Tested on 1.94.1.

```bash
# Build everything
cargo build

# Build release binaries
cargo build --release

# Run all tests
cargo test

# Build the wasm bindings (requires the wasm32 target)
cargo build -p vectize-wasm --target wasm32-unknown-unknown

# Check formatting
cargo fmt --check

# Run linter
cargo clippy --all-targets --all-features -- -D warnings

# Apply auto-formatting
cargo fmt
```

### Test Coverage

The project includes **182 automated tests** across five categories:

| Category          | Location                                      | Tests |
| ----------------- | --------------------------------------------- | ----- |
| Unit tests        | Embedded in each module (`#[cfg(test)]`)      | 117   |
| Integration tests | `crates/vectize/tests/integration_tests.rs`   | 44    |
| CLI smoke tests   | `crates/vectize-cli/tests/cli_smoke_tests.rs` | 19    |
| WASM unit tests   | `crates/vectize-wasm/src/lib.rs`              | 1     |
| Doc tests         | `crates/vectize/src/lib.rs`                   | 1     |

Test types include:

- **API integration tests** – end-to-end tracing from bytes and files
- **Configuration validation** – all presets and error cases
- **Golden / snapshot tests** – deterministic output verification
- **CLI smoke tests** – help text, convert, batch, presets, options, error cases
- **Edge cases** – 1×1 images, max colors, transparent images, no smoothing

The `trace` binary is produced at `target/release/trace` (or `target/debug/trace` for debug builds).

---

## CLI Usage

### Single-file conversion

```bash
# Basic conversion with default settings
trace convert input.png -o output.svg

# Use the high-quality preset
trace convert input.webp --preset high

# Customize palette size, tolerance, smoothing, corners, and despeckling
trace convert photo.jpg --colors 32 --tolerance 0.5 --smoothing 0.7 --corner-sensitivity 0.8 --despeckle-threshold 1.5

# Write SVG to stdout (useful for piping)
trace convert input.png --stdout

# Overwrite an existing output file
trace convert input.png -o output.svg --overwrite

# Enable Gaussian denoising
trace convert noisy.png --denoise

# Set a custom alpha threshold
trace convert transparent.png --alpha-threshold 64

# Composite transparency against a custom background color
trace convert transparent.png --background-color "#102030"

# Disable preprocessing for already-clean source art
trace convert logo.png --no-preprocess

# Approximate smooth fills with SVG gradients
trace convert sky.png --gradients

# Reduce peak segmentation memory on large inputs with tile-aware assignment
trace convert poster.png --preset high --tile-size 512
```

### Batch conversion

```bash
# Convert all PNG/JPEG/WebP files in a directory
trace batch ./images/ ./vectors/ --format svg

# Use a quality preset for the whole batch
trace batch ./images/ ./vectors/ --format svg --preset fast

# Overwrite any existing SVG files
trace batch ./images/ ./vectors/ --format svg --overwrite

# Verbose output
trace batch ./images/ ./vectors/ --format svg -v

# Apply the same tracing overrides to every file in the batch
trace batch ./images/ ./vectors/ --format svg --colors 24 --tolerance 0.6 --denoise

# Batch-convert large files with gradients and tiled segmentation
trace batch ./images/ ./vectors/ --format svg --gradients --tile-size 512
```

### Help

```bash
trace --help
trace convert --help
trace batch --help
```

---

## API Usage (Rust)

Add to `Cargo.toml`:

```toml
[dependencies]
vectize = { path = "crates/vectize" }
```

### Quick start with a quality preset

```rust
use vectize::{Tracer, QualityPreset};

let tracer = Tracer::with_preset(QualityPreset::High);
let result = tracer.trace_file_result("input.png")?;
result.write_svg("output.svg", true)?;
```

### Custom configuration

```rust
use vectize::{Tracer, TracingConfig};

let config = TracingConfig {
    color_count: 24,
    simplification_tolerance: 0.8,
    min_region_area: 3.0,
    smoothing_strength: 0.6,
    enable_denoising: true,
    background_color: Some((0x10, 0x20, 0x30)),
    enable_svg_gradients: true,
    tile_size: Some(512),
    ..TracingConfig::default()
};

let tracer = Tracer::new(config);
let svg = tracer.trace_file("photo.jpg")?;
```

### Start from a preset and apply shared overrides

```rust
use vectize::{QualityPreset, Tracer, TracingConfig, TracingConfigOverrides};

let overrides = TracingConfigOverrides {
    color_count: Some(24),
    background_color: Some((0x10, 0x20, 0x30)),
    enable_svg_gradients: Some(true),
    tile_size: Some(512),
    ..TracingConfigOverrides::default()
};

let config = TracingConfig::from_preset_with_overrides(QualityPreset::High, &overrides)?;
let tracer = Tracer::new(config);
let svg = tracer.trace_file("photo.jpg")?;
```

### Inspect debug-oriented tracing data

```rust
use vectize::{QualityPreset, Tracer};

let tracer = Tracer::with_preset(QualityPreset::Balanced);
let result = tracer.trace_file_result("input.png")?;

println!("palette colors: {}", result.debug().palette().len());
if let Some(metrics) = result.stage_metrics() {
    println!(
    "extracted={} invalid={} after_despeckle={} simplified_away={} filtered_area={} suppressed_background={} emitted={}",
        metrics.contours_extracted(),
      metrics.invalid_contours_discarded(),
        metrics.contours_after_despeckle(),
      metrics.contours_simplified_away(),
      metrics.contours_filtered_min_area(),
      metrics.contours_suppressed_background(),
        metrics.contours_emitted()
    );
}
for region in result.debug().regions() {
    println!(
        "region {} contours={} holes={} points={}",
        region.color().to_hex(),
        region.contour_count(),
        region.hole_count(),
        region.total_points()
    );
}
```

### Trace from bytes (e.g. from an HTTP response)

```rust
use vectize::trace_bytes;

let bytes: Vec<u8> = download_image();
let svg = trace_bytes(&bytes)?;
```

### Observe pipeline progress for UI feedback

```rust
use vectize::{QualityPreset, TraceStage, Tracer};

let tracer = Tracer::with_preset(QualityPreset::High);
let result = tracer.trace_file_result_with_progress("input.png", |update| {
    eprintln!(
        "stage={:?} progress={}/{}",
        update.stage(),
        update.completed_stages(),
        update.total_stages()
    );
})?;

assert_eq!(result.width() > 0, true);
```

### JavaScript / TypeScript via `wasm-bindgen`

The workspace now includes `crates/vectize-wasm`, which exposes:

- `traceBytesSvg(bytes)` → SVG string
- `traceBytes(bytes)` → structured tracing result
- `traceBytesWithConfig(bytes, config)` → structured tracing result with Rust-equivalent options
- `traceBytesWithProgress(bytes, config, callback)` → structured tracing result plus stage callbacks

Example usage after packaging the generated wasm bundle:

```ts
import init, { traceBytesWithConfig, traceBytesWithProgress } from "./pkg/vectize_wasm";

await init();

const result = traceBytesWithConfig(bytes, {
  preset: "high",
  backgroundColor: "#102030",
  enableSvgGradients: true,
  tileSize: 512,
});

traceBytesWithProgress(bytes, { preset: "balanced" }, (update) => {
  console.log(update.stage, update.completedStages, update.totalStages);
});
```

### Validate configuration before use

```rust
use vectize::TracingConfig;

let config = TracingConfig { color_count: 1, ..Default::default() };
if let Err(msg) = config.validate() {
    eprintln!("Bad config: {msg}");
}
```

---

## Pipeline Description

The library processes images in eight sequential stages:

1. **Load** — `pipeline::loader`
  Decode PNG, JPEG, or WebP using the `image` crate. Format is inferred from the
  file extension or byte magic header.
2. **Preprocess** — `pipeline::preprocess`
  Convert to RGBA8, optionally apply a 3×3 edge-aware Gaussian blur for denoising
  (controlled by `enable_preprocessing` and `enable_denoising`), optionally resample
  to a denser tracing grid for higher-fidelity presets, and composite transparent
  pixels against a white background.
3. **Segment** — `pipeline::segment`
  Reduce the palette to *N* colors using deterministic **perceptual Oklab
  quantization** with farthest-point seeding and refinement passes. The segmenter
  collapses anti-aliased bridge shades, compacts unused palette entries, adaptively
  caps flat-art traces to a smaller internal palette, and can assign palette labels
  tile-by-tile on large images to reduce peak segmentation memory.
4. **Contour** — `pipeline::contour`
  Trace deterministic grid-edge contour loops for each color region, preserving
  interior holes and stable winding. Incomplete or otherwise invalid loops are
  discarded before downstream processing and reported through stage metrics for
  diagnostics.
5. **Despeckle** — `pipeline::mod`
  Remove tiny contours whose perimeter falls below `despeckle_threshold`, suppressing
  noise artifacts and speckles.
6. **Simplify** — `pipeline::simplify`
  Reduce polygon point count with the **Ramer-Douglas-Peucker** algorithm. The
  `simplification_tolerance` parameter controls aggressiveness.
7. **Smooth** — `pipeline::smooth`
  Apply **adaptive Laplacian vertex relaxation** to shift grid-aligned contour
  vertices off the integer grid while damping the effect near sharp corners,
  reducing staircase artifacts without rounding away important structure before curve
  fitting. The relaxation weight is derived from `smoothing_strength`, modulated by
  `corner_sensitivity`, and followed by a deterministic area-compensation pass for
  larger contours.
8. **Curves + SVG** — `pipeline::curves`, `pipeline::svg`
  Fit closed contours to **cubic Bezier splines** using wrap-around corner detection
  and edge-aligned handles, build per-region SVG paths concurrently with `rayon`,
  merge same-colored fragments into fewer `<path>` elements, optionally approximate
  smooth fills with linear/radial SVG gradients, then emit valid SVG markup with
  deterministic area-based ordering so smaller details stay above larger fills.

Border-connected pure-white background contours are omitted from final `<path>` elements
because the document already includes a white background rectangle. Interior white islands
are still emitted normally, including islands that share the same white palette entry as
the suppressed border-connected background.

All SVG path coordinates are serialized with fixed two-decimal precision so linear and
Bezier output use the same deterministic coordinate format.

Bezier control points are clamped to the canvas bounds so edge-touching smoothed paths
stay inside the final SVG viewBox.

Structured stage metrics also report contours simplified away during SVG preparation,
contours filtered by `min_region_area`, and contours suppressed as redundant background.

When using the `high` preset, tracing runs on an internal **2× resampled grid** and maps the
resulting geometry back into the original SVG viewBox. This improves edge placement and retains
more small features, while the segmentation stage now automatically caps flat graphics to a
smaller internal palette when a large color budget would only preserve anti-aliased fringe shades.

---

## Configuration Reference

- `color_count` (`u16`, default `16`) — Number of palette colors (2–256)
- `simplification_tolerance` (`f64`, default `1.0`) — RDP tolerance in pixels
- `min_region_area` (`f64`, default `4.0`) — Minimum polygon area to include
- `smoothing_strength` (`f64`, default `0.5`) — Contour smoothing + Bezier curves
  (`0` = straight lines)
- `corner_sensitivity` (`f64`, default `0.6`) — Corner preservation strength for
  adaptive smoothing and Bezier fitting
- `alpha_threshold` (`u8`, default `128`) — Pixels below this alpha are transparent
- `despeckle_threshold` (`f64`, default `2.0`) — Minimum contour perimeter
- `enable_denoising` (`bool`, default `false`) — Apply edge-aware Gaussian blur
  before tracing
- `enable_preprocessing` (`bool`, default `true`) — Enable normalization stage
- `background_color` (`Option<(u8, u8, u8)>`, default `None`) — Composite transparency
  and emit the SVG background using a custom RGB color; `None` falls back to white
- `enable_svg_gradients` (`bool`, default `false`) — Approximate smooth region fills
  with linear/radial SVG gradients
- `tile_size` (`Option<u32>`, default `None`) — Assign palette labels in tiles to
  reduce peak segmentation memory

### Quality Presets

| Preset     | Colors | Tolerance | Denoising | Use case                   |
| ---------- | ------ | --------- | --------- | -------------------------- |
| `fast`     | 8      | 2.0       | off       | Quick previews, low detail |
| `balanced` | 16     | 1.0       | off       | General-purpose (default)  |
| `high`     | 64*    | 1.2       | on        | Maximum fidelity           |

`*` The `high` preset still accepts up to 64 colors, but simple flat graphics are internally
capped to a smaller palette when the extra buckets would mostly preserve anti-aliased edge shades
instead of meaningful regions. Because `high` traces on an internal 2× grid, the `1.2` tolerance
still corresponds to a tighter effective output-space simplification than the default `balanced`
preset.

---

## Dependency Choices

- [`image`](https://crates.io/crates/image) — De facto standard Rust image I/O;
  handles PNG, JPEG, and WebP decoding with a unified API.
- [`thiserror`](https://crates.io/crates/thiserror) — Generates `Display` and
  `Error` impls for the error enum with minimal boilerplate.
- [`log`](https://crates.io/crates/log) — Logging facade that keeps the library
  decoupled from any specific logger backend.
- [`rayon`](https://crates.io/crates/rayon) — Work-stealing parallel iterator
  library used for CLI batch conversion and deterministic per-region SVG path
  construction.
- [`clap`](https://crates.io/crates/clap) *(CLI only)* — Industry-standard
  argument parsing with derive macros for clean, self-documenting CLI definitions.
- [`env_logger`](https://crates.io/crates/env_logger) *(CLI only)* — Simple
  `RUST_LOG`-driven logger backend for the CLI binary.
- [`wasm-bindgen`](https://crates.io/crates/wasm-bindgen) *(WASM crate only)* —
  Exposes the tracer to JavaScript/TypeScript without changing the native Rust API
  surface.
- [`js-sys`](https://crates.io/crates/js-sys) *(WASM crate only)* — Provides JS
  callback and typed-array interop for progress reporting and byte input.
- [`serde-wasm-bindgen`](https://crates.io/crates/serde-wasm-bindgen) *(WASM crate
  only)* — Converts rich tracing results and config objects between Rust and JS
  values.

---

## Limitations and Tradeoffs

- **Pixel-grid coordinates.** Contours are traced on integer pixel coordinates. Adaptive Laplacian
  vertex smoothing shifts points off-grid before Bezier fitting, but very small images may
  still produce slightly blocky results even at high quality settings.
- **Tile mode reduces segmentation memory, not decode memory.** The `image` crate still decodes
  the full source eagerly before the tile-aware palette-assignment pass begins.
- **Gradient fitting is intentionally conservative.** Busy textures and sharp color steps stay as
  solid fills; gradients are only emitted when a deterministic linear/radial fit clearly helps.
- **WebP encoding is not supported** — only decoding. The output is always SVG.

---

## License

MIT — see [LICENSE](LICENSE).
