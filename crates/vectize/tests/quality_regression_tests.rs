use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use vectize::{QualityPreset, Tracer, TracingResult};

static FIXTURE_TRACE_LOCK: Mutex<()> = Mutex::new(());
static INPUT1_RESULT: OnceLock<TracingResult> = OnceLock::new();
static INPUT5_RESULT: OnceLock<TracingResult> = OnceLock::new();
static INPUT6_RESULT: OnceLock<TracingResult> = OnceLock::new();

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
        "input5.jpg" => INPUT5_RESULT.get_or_init(|| trace_high_fixture(name)),
        "input6.jpg" => INPUT6_RESULT.get_or_init(|| trace_high_fixture(name)),
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
