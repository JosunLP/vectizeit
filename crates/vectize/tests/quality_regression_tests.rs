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

#[derive(Debug, Clone, PartialEq)]
struct SvgPathAttributes {
    fill: String,
    stroke: Option<String>,
    stroke_width: Option<f64>,
    data: String,
}

fn extract_tag_attribute(tag: &str, attribute: &str) -> Option<String> {
    let needle = format!(r#"{attribute}=""#);
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')?;
    Some(tag[start..start + end].to_string())
}

fn extract_viewbox_dimensions(svg: &str) -> Option<(f64, f64)> {
    let viewbox = extract_attribute_values(svg, "viewBox")
        .into_iter()
        .next()?;
    let mut parts = viewbox.split_whitespace();
    let _min_x = parts.next()?.parse::<f64>().ok()?;
    let _min_y = parts.next()?.parse::<f64>().ok()?;
    let width = parts.next()?.parse::<f64>().ok()?;
    let height = parts.next()?.parse::<f64>().ok()?;
    Some((width, height))
}

fn extract_path_attributes(svg: &str) -> Vec<SvgPathAttributes> {
    let mut paths = Vec::new();
    let mut search_offset = 0;

    while let Some(relative_start) = svg[search_offset..].find("<path ") {
        let tag_start = search_offset + relative_start;
        let Some(relative_end) = svg[tag_start..].find("/>") else {
            break;
        };
        let tag_end = tag_start + relative_end + 2;
        let tag = &svg[tag_start..tag_end];

        paths.push(SvgPathAttributes {
            fill: extract_tag_attribute(tag, "fill").unwrap_or_default(),
            stroke: extract_tag_attribute(tag, "stroke"),
            stroke_width: extract_tag_attribute(tag, "stroke-width")
                .and_then(|value| value.parse::<f64>().ok()),
            data: extract_tag_attribute(tag, "d").unwrap_or_default(),
        });

        search_offset = tag_end;
    }

    paths
}

fn extract_path_data(svg: &str) -> Vec<String> {
    extract_path_attributes(svg)
        .into_iter()
        .map(|path| path.data)
        .collect()
}

fn ordered_unique_path_fills(svg: &str) -> Vec<String> {
    let mut ordered = Vec::new();

    for fill in extract_path_attributes(svg)
        .into_iter()
        .map(|path| path.fill)
    {
        if !ordered.contains(&fill) {
            ordered.push(fill);
        }
    }

    ordered
}

fn average_stroke_width(svg: &str) -> f64 {
    let stroke_widths: Vec<_> = extract_path_attributes(svg)
        .into_iter()
        .filter(|path| {
            path.stroke
                .as_deref()
                .is_some_and(|stroke| stroke != "none")
        })
        .filter_map(|path| path.stroke_width)
        .collect();

    if stroke_widths.is_empty() {
        0.0
    } else {
        stroke_widths.iter().sum::<f64>() / stroke_widths.len() as f64
    }
}

fn count_paths_with_min_stroke_width(svg: &str, min_stroke_width: f64) -> usize {
    extract_path_attributes(svg)
        .into_iter()
        .filter(|path| {
            path.stroke
                .as_deref()
                .is_some_and(|stroke| stroke != "none")
        })
        .filter(|path| {
            path.stroke_width
                .is_some_and(|stroke_width| stroke_width >= min_stroke_width)
        })
        .count()
}

fn is_dark_neutral_fill(fill: &str) -> bool {
    if fill.len() != 7 || !fill.starts_with('#') {
        return false;
    }

    let red = u8::from_str_radix(&fill[1..3], 16).ok();
    let green = u8::from_str_radix(&fill[3..5], 16).ok();
    let blue = u8::from_str_radix(&fill[5..7], 16).ok();
    let (Some(red), Some(green), Some(blue)) = (red, green, blue) else {
        return false;
    };

    let max_channel = red.max(green).max(blue);
    let min_channel = red.min(green).min(blue);
    let luminance =
        (0.2126 * f64::from(red)) + (0.7152 * f64::from(green)) + (0.0722 * f64::from(blue));

    luminance <= 42.0 && max_channel.saturating_sub(min_channel) <= 20
}

fn count_outline_like_paths(svg: &str) -> usize {
    extract_path_attributes(svg)
        .into_iter()
        .filter(|path| is_dark_neutral_fill(&path.fill))
        .count()
}

fn count_outline_like_stroked_paths(svg: &str) -> usize {
    extract_path_attributes(svg)
        .into_iter()
        .filter(|path| is_dark_neutral_fill(&path.fill))
        .filter(|path| {
            path.stroke
                .as_deref()
                .is_some_and(|stroke| stroke != "none")
        })
        .count()
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
fn graphics_svg_helpers_extract_viewbox_paths_and_fill_order() {
    let svg = r##"<svg viewBox="0 0 91 51"><path fill="#111111" stroke="none" d="M 0 0 Z"/><path fill="#222222" stroke="#222222" stroke-width="0.12" d="M 1 1 Z"/></svg>"##;

    assert_eq!(extract_viewbox_dimensions(svg), Some((91.0, 51.0)));
    assert_eq!(
        extract_path_data(svg),
        vec!["M 0 0 Z".to_string(), "M 1 1 Z".to_string()]
    );
    assert_eq!(
        ordered_unique_path_fills(svg),
        vec!["#111111".to_string(), "#222222".to_string()]
    );
    assert_eq!(average_stroke_width(svg), 0.12);
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
        count_outline_like_paths(svg) >= 1,
        "input2 should keep at least one dark outline-like shield path"
    );
    assert_eq!(
        count_outline_like_stroked_paths(svg),
        0,
        "input2 outline-like shield paths should rely on overlap, not seam strokes"
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
        count_outline_like_paths(svg) >= 2,
        "input3 should keep multiple dark neutral letter/bar regions intact"
    );
    assert_eq!(
        count_outline_like_stroked_paths(svg),
        0,
        "input3 dark text regions should stay seam-free to avoid lettering fattening"
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
        "input6 should stay below the dense-region emission budget (got {})",
        metrics.regions_emitted()
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
        average_stroke_width(svg) <= 0.15 + f64::EPSILON,
        "input7 should bias seam strokes toward micro-seams instead of visibly fattening thin geometry"
    );
    assert!(
        count_paths_with_min_stroke_width(svg, 0.16) <= 8,
        "input7 should keep the number of wide seam strokes on the waveform/logo under control"
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

    assert_eq!(
        extract_viewbox_dimensions(svg),
        Some((91.0, 51.0)),
        "input8 should preserve the tiny canvas dimensions exactly"
    );
    assert_eq!(
        ordered_unique_path_fills(svg),
        vec![
            "#fadd3c".to_string(),
            "#ab38b8".to_string(),
            "#ed1d24".to_string(),
            "#ff8129".to_string(),
            "#4093e5".to_string(),
            "#17e77a".to_string(),
            "#684634".to_string(),
            "#7dc7fd".to_string(),
            "#f28da9".to_string(),
            "#fffdfa".to_string(),
            "#000000".to_string(),
        ],
        "input8 should preserve the tiny flag's band and chevron color order"
    );
    assert!(
        unique_hex_fill_count(svg) >= 10,
        "input8 should keep the small-canvas stripe and chevron colors distinct"
    );
    assert_eq!(
        count_outline_like_stroked_paths(svg),
        0,
        "input8 outline-like chevron borders should stay seam-free on the tiny canvas"
    );
    assert!(
        count_paths_with_min_stroke_width(svg, 0.14) <= 1,
        "input8 should keep only the broadest tiny-canvas seams at or above 0.14px"
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
