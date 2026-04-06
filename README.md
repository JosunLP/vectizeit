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
│   │   │       ├── curves.rs
│   │   │       └── svg.rs
│   │   └── tests/
│   │       └── integration_tests.rs  # API & golden tests (27 tests)
│   └── vectize-cli/            # CLI binary crate (`trace`)
│       ├── src/main.rs
│       └── tests/
│           └── cli_smoke_tests.rs    # CLI smoke tests (17 tests)
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

# Check formatting
cargo fmt --check

# Run linter
cargo clippy -- -D warnings

# Apply auto-formatting
cargo fmt
```

### Test Coverage

The project includes **76 automated tests** across four categories:

| Category          | Location                                      | Tests |
| ----------------- | --------------------------------------------- | ----- |
| Unit tests        | Embedded in each module (`#[cfg(test)]`)      | 31    |
| Integration tests | `crates/vectize/tests/integration_tests.rs`   | 27    |
| CLI smoke tests   | `crates/vectize-cli/tests/cli_smoke_tests.rs` | 17    |
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

# Disable preprocessing for already-clean source art
trace convert logo.png --no-preprocess
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
    ..TracingConfig::default()
};

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
        "extracted={} after_despeckle={} emitted={}",
        metrics.contours_extracted(),
        metrics.contours_after_despeckle(),
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

The library processes images in seven sequential stages:

| Stage           | Module                              | Description                                                                                                                                                                                                                                          |
| --------------- | ----------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1. Load         | `pipeline::loader`                  | Decode PNG, JPEG, or WebP using the `image` crate. Format is inferred from the file extension or byte magic header.                                                                                                                                  |
| 2. Preprocess   | `pipeline::preprocess`              | Convert to RGBA8, optionally apply a 3×3 Gaussian blur for denoising (controlled by `enable_preprocessing` and `enable_denoising`), and composite transparent pixels against a white background.                                                     |
| 3. Segment      | `pipeline::segment`                 | Reduce the palette to *N* colors using **median-cut quantization**. Each pixel is assigned the index of its nearest palette entry in RGB space.                                                                                                      |
| 4. Contour      | `pipeline::contour`                 | Trace deterministic grid-edge contour loops for each color region, preserving interior holes and stable winding.                                                                                                                                     |
| 5. Despeckle    | `pipeline::mod`                     | Remove tiny contours whose perimeter falls below `despeckle_threshold`, suppressing noise artifacts and speckles.                                                                                                                                    |
| 6. Simplify     | `pipeline::simplify`                | Reduce polygon point count with the **Ramer-Douglas-Peucker** algorithm. The `simplification_tolerance` parameter controls aggressiveness.                                                                                                           |
| 7. Curves + SVG | `pipeline::curves`, `pipeline::svg` | Smooth closed contours into **cubic Bezier splines** using Catmull-Rom tangents with **wrap-around corner detection** (`corner_sensitivity`), then emit valid SVG markup with proper `viewBox`, `<path>` elements, and a white background rectangle. |

---

## Configuration Reference

| Field                      | Type   | Default | Description                             |
| -------------------------- | ------ | ------- | --------------------------------------- |
| `color_count`              | `u16`  | `16`    | Number of palette colors (2–256)        |
| `simplification_tolerance` | `f64`  | `1.0`   | RDP tolerance in pixels                 |
| `min_region_area`          | `f64`  | `4.0`   | Minimum polygon area to include         |
| `smoothing_strength`       | `f64`  | `0.5`   | Bezier smoothing (0 = straight lines)   |
| `corner_sensitivity`       | `f64`  | `0.6`   | Corner preservation threshold           |
| `alpha_threshold`          | `u8`   | `128`   | Pixels below this alpha are transparent |
| `despeckle_threshold`      | `f64`  | `2.0`   | Minimum contour perimeter               |
| `enable_denoising`         | `bool` | `false` | Apply Gaussian blur before tracing      |
| `enable_preprocessing`     | `bool` | `true`  | Enable normalization stage              |

### Quality Presets

| Preset     | Colors | Tolerance | Denoising | Use case                   |
| ---------- | ------ | --------- | --------- | -------------------------- |
| `fast`     | 8      | 2.0       | off       | Quick previews, low detail |
| `balanced` | 16     | 1.0       | off       | General-purpose (default)  |
| `high`     | 32     | 0.3       | on        | Maximum fidelity           |

---

## Dependency Choices

| Crate                                                          | Reason                                                                                                 |
| -------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| [`image`](https://crates.io/crates/image)                      | De facto standard Rust image I/O; handles PNG, JPEG, and WebP decoding with a unified API.             |
| [`thiserror`](https://crates.io/crates/thiserror)              | Generates `Display` and `Error` impls for the error enum with minimal boilerplate.                     |
| [`log`](https://crates.io/crates/log)                          | Logging facade — keeps the library decoupled from any specific logger backend.                         |
| [`rayon`](https://crates.io/crates/rayon)                      | Work-stealing parallel iterator library; declared for future parallelization of per-region processing. |
| [`clap`](https://crates.io/crates/clap) (CLI only)             | Industry-standard argument parsing with derive macros for clean, self-documenting CLI definitions.     |
| [`env_logger`](https://crates.io/crates/env_logger) (CLI only) | Simple `RUST_LOG`-driven logger backend for the CLI binary.                                            |

---

## Limitations and Tradeoffs

- **Pixel-grid coordinates.** Contours operate on integer pixel coordinates, so very small
  images may produce blocky results even at high quality settings.
- **No path merging.** Adjacent regions of the same color from different quantization buckets
  are emitted as separate paths rather than being merged.
- **Median-cut quantization** can sometimes split perceptually similar colors and merge
  distinct ones compared to perceptual quantizers (e.g. neuquant). It is fast and
  deterministic, which makes it suitable for a library.
- **No streaming input.** The entire image is loaded into memory before processing begins.
- **WebP encoding is not supported** — only decoding. The output is always SVG.

---

## Future Improvements

- [ ] **Parallel region processing** — use `rayon` to process color regions concurrently.
- [ ] **Perceptual color quantization** — replace median-cut with a perceptually-uniform
  palette (e.g. Wu quantization or OCIAF).
- [ ] **Adaptive smoothing** — apply higher smoothing to gently-curved segments and lower
  smoothing near detected corners.
- [ ] **Path merging** — merge adjacent same-colored paths into a single path element.
- [ ] **WASM target** — expose the library to JavaScript/TypeScript via `wasm-bindgen`.
- [ ] **Progress callbacks** — let callers observe pipeline stage completion for UI feedback.
- [ ] **SVG gradient support** — approximate smooth color gradients with radial/linear SVG
  gradient elements.
- [ ] **Streaming / tiled processing** — process large images in tiles to reduce peak memory
  usage.

---

## License

MIT — see [LICENSE](LICENSE).
