use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use vectize::{QualityPreset, Tracer, TracingResult};

static FIXTURE_TRACE_LOCK: Mutex<()> = Mutex::new(());
static INPUT1_RESULT: OnceLock<TracingResult> = OnceLock::new();
static INPUT2_RESULT: OnceLock<TracingResult> = OnceLock::new();
static INPUT3_RESULT: OnceLock<TracingResult> = OnceLock::new();
static INPUT4_RESULT: OnceLock<TracingResult> = OnceLock::new();
static INPUT5_RESULT: OnceLock<TracingResult> = OnceLock::new();
static INPUT6_RESULT: OnceLock<TracingResult> = OnceLock::new();
static INPUT7_RESULT: OnceLock<TracingResult> = OnceLock::new();
static INPUT8_RESULT: OnceLock<TracingResult> = OnceLock::new();

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("testfiles")
        .join(name)
}

fn trace_high_fixture(name: &str) -> TracingResult {
    let _guard = FIXTURE_TRACE_LOCK
        .lock()
        .expect("fixture trace lock should not be poisoned");
    let path = fixture_path(name);
    assert!(
        path.is_file(),
        "expected fixture '{name}' at {}",
        path.display()
    );

    Tracer::with_preset(QualityPreset::High)
        .trace_file_result(&path)
        .unwrap_or_else(|err| panic!("failed to trace fixture '{}': {err}", path.display()))
}

fn cached_fixture_result(name: &str) -> &'static TracingResult {
    match name {
        "input1.png" => INPUT1_RESULT.get_or_init(|| trace_high_fixture(name)),
        "input2.png" => INPUT2_RESULT.get_or_init(|| trace_high_fixture(name)),
        "input3.jpg" => INPUT3_RESULT.get_or_init(|| trace_high_fixture(name)),
        "input4.png" => INPUT4_RESULT.get_or_init(|| trace_high_fixture(name)),
        "input5.jpg" => INPUT5_RESULT.get_or_init(|| trace_high_fixture(name)),
        "input6.jpg" => INPUT6_RESULT.get_or_init(|| trace_high_fixture(name)),
        "input7.png" => INPUT7_RESULT.get_or_init(|| trace_high_fixture(name)),
        "input8.png" => INPUT8_RESULT.get_or_init(|| trace_high_fixture(name)),
        other => panic!("no cache slot configured for fixture '{other}'"),
    }
}

fn extract_attribute_values(svg: &str, attribute: &str) -> Vec<String> {
    let needle = format!(r#"{attribute}=""#);
    let mut values = Vec::new();
    let mut search_offset = 0;

    while let Some(relative_start) = svg[search_offset..].find(&needle) {
        let value_start = search_offset + relative_start + needle.len();
        let Some(relative_end) = svg[value_start..].find('"') else {
            break;
        };

        values.push(svg[value_start..value_start + relative_end].to_string());
        search_offset = value_start + relative_end + 1;
    }

    values
}

fn extract_decimal_attribute_values(svg: &str, attribute: &str) -> Vec<f64> {
    extract_attribute_values(svg, attribute)
        .into_iter()
        .filter_map(|value| value.parse::<f64>().ok())
        .collect()
}

fn unique_hex_fill_count(svg: &str) -> usize {
    let mut values: Vec<_> = extract_attribute_values(svg, "fill")
        .into_iter()
        .filter(|value| value.starts_with('#'))
        .collect();
    values.sort();
    values.dedup();
    values.len()
}

fn max_stroke_width(svg: &str) -> f64 {
    extract_decimal_attribute_values(svg, "stroke-width")
        .into_iter()
        .fold(0.0, f64::max)
}

fn fill_count(svg: &str, fill: &str) -> usize {
    svg.matches(&format!(r#"fill="{fill}""#)).count()
}

#[test]
fn fixture_input1_high_keeps_logo_detail_without_heavy_seams() {
    let result = cached_fixture_result("input1.png");
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");
    let svg = result.svg();

    assert!(
        fill_count(svg, "#ffffff") >= 3,
        "input1 should keep multiple bright logo/text regions instead of collapsing them"
    );
    assert!(
        unique_hex_fill_count(svg) >= 20,
        "input1 should preserve the logo's distinct fill bands"
    );
    assert!(
        metrics.regions_emitted() >= 20,
        "input1 should retain multiple emitted regions for the logo structure"
    );
    assert!(
        metrics.contours_emitted() <= 120,
        "input1 should not explode into excessive contour fragments"
    );
    assert!(
        max_stroke_width(svg) <= 0.20 + f64::EPSILON,
        "input1 seam-closing strokes should stay tiny"
    );
}

#[test]
fn fixture_input2_high_keeps_nested_heraldic_structure_compact() {
    let result = cached_fixture_result("input2.png");
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");
    let svg = result.svg();

    assert!(
        unique_hex_fill_count(svg) >= 6,
        "input2 should preserve the shield, crest, foliage, and wave color structure"
    );
    assert!(
        fill_count(svg, "#ffffff") >= 2,
        "input2 should keep the interior white heraldic spaces distinct from the background"
    );
    assert!(
        metrics.regions_emitted() <= 14,
        "input2 should not fragment the heraldic regions into too many separate paths"
    );
    assert!(
        metrics.contours_emitted() <= 140,
        "input2 should stay below the heraldic contour budget"
    );
    assert!(
        metrics.holes_emitted() >= 12,
        "input2 should preserve nested shield cutouts and interior wave gaps"
    );
    assert!(
        metrics.points_emitted() <= 5_000,
        "input2 should stay within the heraldic point budget"
    );
    assert!(
        max_stroke_width(svg) <= 0.20 + f64::EPSILON,
        "input2 seam-closing strokes should stay tiny"
    );
}

#[test]
fn fixture_input3_high_keeps_bold_text_holes_without_jagged_fragmentation() {
    let result = cached_fixture_result("input3.jpg");
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");
    let svg = result.svg();

    assert!(
        unique_hex_fill_count(svg) >= 6,
        "input3 should preserve the black/white/red logo palette instead of collapsing it"
    );
    assert!(
        fill_count(svg, "#ffffff") >= 2,
        "input3 should keep the white lettering distinct from the background"
    );
    assert!(
        metrics.regions_emitted() <= 8,
        "input3 should not fragment the bold text and bars into too many paths"
    );
    assert!(
        metrics.contours_emitted() <= 120,
        "input3 should stay below the bold-logo contour budget"
    );
    assert!(
        metrics.holes_emitted() >= 8,
        "input3 should preserve the counters and cutouts inside the bold lettering"
    );
    assert!(
        metrics.points_emitted() <= 2_200,
        "input3 should stay within the bold-logo point budget"
    );
    assert!(
        max_stroke_width(svg) <= 0.20 + f64::EPSILON,
        "input3 seam-closing strokes should stay tiny"
    );
}

#[test]
fn fixture_input5_high_limits_fragment_explosion_without_palette_collapse() {
    let result = cached_fixture_result("input5.jpg");
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");
    let svg = result.svg();

    assert!(
        unique_hex_fill_count(svg) >= 60,
        "input5 should keep a rich set of fills after vectorization"
    );
    assert!(
        metrics.contours_simplified_away() >= 20_000,
        "input5 should simplify away a large number of tiny stair-step contours"
    );
    assert!(
        metrics.contours_emitted() <= 6_500,
        "input5 should stay below the dense-fragment contour budget"
    );
    assert!(
        metrics.points_emitted() <= 70_000,
        "input5 should stay below the dense-fragment point budget"
    );
    assert!(
        max_stroke_width(svg) <= 0.20 + f64::EPSILON,
        "input5 seam-closing strokes should stay tiny"
    );
}

#[test]
fn fixture_input6_high_keeps_emblem_detail_within_dense_output_budget() {
    let result = cached_fixture_result("input6.jpg");
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");
    let svg = result.svg();

    assert!(
        unique_hex_fill_count(svg) >= 60,
        "input6 should keep a rich set of highlight/shadow fills"
    );
    assert!(
        metrics.contours_simplified_away() >= 110_000,
        "input6 should simplify away a very large number of dense stair-step contours"
    );
    assert!(
        metrics.regions_emitted() <= 340,
        "input6 should stay below the dense-region emission budget"
    );
    assert!(
        metrics.contours_emitted() <= 8_000,
        "input6 should stay below the dense-contour emission budget"
    );
    assert!(
        metrics.holes_emitted() <= 2_200,
        "input6 should not regress back into excessive hole fragmentation"
    );
    assert!(
        metrics.points_emitted() <= 180_000,
        "input6 should stay below the dense-point emission budget"
    );
    assert!(
        max_stroke_width(svg) <= 0.20 + f64::EPSILON,
        "input6 seam-closing strokes should stay tiny"
    );
}

#[test]
fn fixture_input7_high_keeps_waveform_and_text_without_outline_fattening() {
    let result = cached_fixture_result("input7.png");
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");
    let svg = result.svg();

    assert!(
        unique_hex_fill_count(svg) >= 10,
        "input7 should keep the blue logo bands and text shading distinct"
    );
    assert!(
        metrics.regions_emitted() <= 40,
        "input7 should not fragment the waveform/logo into too many path groups"
    );
    assert!(
        metrics.contours_emitted() <= 520,
        "input7 should stay below the thin-line contour budget"
    );
    assert!(
        metrics.holes_emitted() >= 60,
        "input7 should preserve the node cutouts and interior text holes"
    );
    assert!(
        metrics.points_emitted() <= 9_500,
        "input7 should stay within the thin-line point budget"
    );
    assert!(
        max_stroke_width(svg) <= 0.20 + f64::EPSILON,
        "input7 seam-closing strokes should stay tiny"
    );
}

#[test]
fn fixture_input8_high_keeps_small_flag_crisp_without_scale_bloat() {
    let result = cached_fixture_result("input8.png");
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");
    let svg = result.svg();

    assert!(
        unique_hex_fill_count(svg) >= 10,
        "input8 should keep the small-canvas stripe and chevron colors distinct"
    );
    assert!(
        metrics.regions_emitted() <= 16,
        "input8 should not over-fragment the small-canvas flag shapes"
    );
    assert!(
        metrics.contours_emitted() <= 20,
        "input8 should stay below the small-canvas contour budget"
    );
    assert!(
        metrics.holes_emitted() <= 4,
        "input8 should not introduce extra hole fragmentation on the tiny canvas"
    );
    assert!(
        metrics.points_emitted() <= 220,
        "input8 should stay within the tiny-canvas point budget"
    );
    assert!(
        max_stroke_width(svg) <= 0.14 + f64::EPSILON,
        "input8 seam-closing strokes should stay extra small on the tiny canvas"
    );
}

#[test]
fn fixture_input4_high_keeps_large_stress_input_within_geometry_budget() {
    let result = cached_fixture_result("input4.png");
    let metrics = result
        .stage_metrics()
        .expect("stage metrics should be present");
    let svg = result.svg();

    assert!(
        unique_hex_fill_count(svg) >= 10,
        "input4 should retain a rich fill set on the large stress input"
    );
    assert!(
        metrics.regions_emitted() <= 48,
        "input4 should stay below the large-image region budget"
    );
    assert!(
        metrics.contours_emitted() <= 820,
        "input4 should stay below the large-image contour budget"
    );
    assert!(
        metrics.holes_emitted() <= 320,
        "input4 should stay below the large-image hole budget"
    );
    assert!(
        metrics.points_emitted() <= 18_000,
        "input4 should stay below the large-image point budget"
    );
    assert!(
        max_stroke_width(svg) <= 0.20 + f64::EPSILON,
        "input4 seam-closing strokes should stay tiny"
    );
}
