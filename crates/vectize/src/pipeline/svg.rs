//! SVG generation from traced contours and Bezier paths.
//!
//! Produces valid, clean SVG markup with proper viewBox, hole-preserving path
//! elements, deterministic output, and fixed coordinate precision across
//! linear and Bezier path serialization.

use rayon::prelude::*;

use crate::config::{QualityPreset, TracingConfig};
use crate::pipeline::contour::{contour_is_hole, signed_area, Contour, Point};
use crate::pipeline::curves::{corner_cosine, fit_closed_cubic_beziers_f64, CubicBezier};
use crate::pipeline::gradient::{SvgGradientKind, SvgGradientPaintMap};
use crate::pipeline::segment::PaletteColor;
use crate::pipeline::simplify::{simplify_closed, simplify_closed_f64};
use crate::pipeline::smooth::smooth_closed_contour_adaptive_multi;

/// A color region consisting of a palette color and its contours.
pub struct ColorRegion {
    pub color: PaletteColor,
    pub contours: Vec<Contour>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SvgEmissionMetrics {
    pub regions_emitted: usize,
    pub contours_emitted: usize,
    pub holes_emitted: usize,
    pub points_emitted: usize,
    pub contours_simplified_away: usize,
    pub contours_filtered_min_area: usize,
    pub contours_suppressed_background: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SvgBuildResult {
    pub svg: String,
    pub metrics: SvgEmissionMetrics,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct PathBuildResult {
    data: String,
    contours_emitted: usize,
    holes_emitted: usize,
    points_emitted: usize,
    contours_simplified_away: usize,
    contours_filtered_min_area: usize,
    seam_stroke_width_centi_px: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RegionBuildResult {
    paths: Vec<PathBuildResult>,
    contours_suppressed_background: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct PathGeometry {
    data: String,
    emitted_points: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct ContourGeometryProfile {
    smoothing_strength: f64,
    corner_sensitivity: f64,
    seam_stroke_width_centi_px: u8,
    post_smoothing_tolerance_factor: f64,
    trace_grid_tolerance_boost: f64,
    prefer_linear: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct ContourEdgeProfile {
    point_density: f64,
    polygonal_factor: f64,
    staircase_factor: f64,
}

#[derive(Clone, Copy)]
struct OrderedRegion<'a> {
    index: usize,
    fill_area: f64,
    region: &'a ColorRegion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OrderedRegionOutput {
    color: PaletteColor,
    hex: String,
    result: RegionBuildResult,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RegionPathPlan {
    contour_groups: Vec<Vec<usize>>,
    contours_suppressed_background: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ContourHierarchyNode {
    parent: Option<usize>,
    children: Vec<usize>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ContourBounds {
    min_x: i32,
    min_y: i32,
    max_x: i32,
    max_y: i32,
}

const SVG_COORD_PRECISION: usize = 2;
const THIN_CONTOUR_THICKNESS_PX: f64 = 2.5;
const LINEAR_CONTOUR_THICKNESS_PX: f64 = 5.5;
const FULL_SMOOTHING_THICKNESS_PX: f64 = 10.0;
const MIN_THIN_CONTOUR_SMOOTHING_FACTOR: f64 = 0.08;
const MIN_SEAM_STROKE_THICKNESS_PX: f64 = 4.5;
const FULL_SEAM_STROKE_THICKNESS_PX: f64 = 12.0;
const MIN_SEAM_STROKE_CENTI_PX: f64 = 8.0;
const MAX_SEAM_STROKE_CENTI_PX: f64 = 20.0;
const DENSE_TRACE_MIN_SEAM_STROKE_CENTI_PX: f64 = 4.0;
const DENSE_TRACE_MAX_SEAM_STROKE_CENTI_PX: f64 = 10.0;
const DENSE_TRACE_SEAM_WIDTH_QUANTIZATION_STEP_CENTI_PX: u8 = 2;
const DENSE_TRACE_MEDIUM_BAND_MAX_SEAM_STROKE_CENTI_PX: u8 = 14;
const SHARP_CORNER_COS_THRESHOLD: f64 = 0.35;
const POLYGONAL_POINT_DENSITY_LOW: f64 = 0.05;
const POLYGONAL_POINT_DENSITY_HIGH: f64 = 0.14;
const STAIRCASE_POINT_DENSITY_LOW: f64 = 0.12;
const STAIRCASE_POINT_DENSITY_HIGH: f64 = 0.28;
const HIGH_POLYGONAL_SMOOTHING_REDUCTION_MAX: f64 = 0.55;
const HIGH_STAIRCASE_SMOOTHING_REDUCTION_MAX: f64 = 0.18;
const HIGH_BASE_SEAM_BONUS_CENTI_PX: f64 = 6.0;
const HIGH_STAIRCASE_SEAM_BONUS_CENTI_PX: f64 = 10.0;
const HIGH_POLYGONAL_LINEAR_FACTOR_MIN: f64 = 0.85;
const HIGH_POLYGONAL_LINEAR_POINT_DENSITY_MAX: f64 = 0.06;
const POST_SMOOTHING_THIN_TOLERANCE_FACTOR: f64 = 0.55;
const POST_SMOOTHING_MEDIUM_TOLERANCE_FACTOR: f64 = 0.72;
const POST_SMOOTHING_STAIRCASE_TOLERANCE_FACTOR: f64 = 0.82;
const HIGH_STAIRCASE_SEAM_GATE_FACTOR: f64 = 0.45;
const HIGH_STAIRCASE_SEAM_BONUS_SCALE: f64 = 0.35;
const MEDIUM_BAND_SEAM_PENALTY_MIN: f64 = 0.55;
const HIGH_TRACE_GRID_TOLERANCE_BOOST_BASE: f64 = 0.18;
const HIGH_TRACE_GRID_TOLERANCE_BOOST_STAIRCASE_MAX: f64 = 0.62;
const OUTLINE_COLOR_LUMA_MAX: f64 = 42.0;
const OUTLINE_COLOR_CHROMA_MAX: u8 = 20;

/// Generate an SVG document from color regions.
pub fn generate_svg(
    regions: &[ColorRegion],
    width: u32,
    height: u32,
    config: &TracingConfig,
) -> String {
    generate_svg_with_metrics(regions, width, height, config).svg
}

pub(crate) fn generate_svg_with_metrics(
    regions: &[ColorRegion],
    width: u32,
    height: u32,
    config: &TracingConfig,
) -> SvgBuildResult {
    generate_svg_with_metrics_and_paints(regions, width, height, config, None)
}

pub(crate) fn generate_svg_with_metrics_and_paints(
    regions: &[ColorRegion],
    width: u32,
    height: u32,
    config: &TracingConfig,
    gradient_paints: Option<&SvgGradientPaintMap>,
) -> SvgBuildResult {
    generate_svg_with_trace_space_and_paints(
        regions,
        width,
        height,
        width,
        height,
        1.0,
        config,
        gradient_paints,
    )
}

#[allow(dead_code)]
pub(crate) fn generate_svg_with_trace_space(
    regions: &[ColorRegion],
    trace_width: u32,
    trace_height: u32,
    output_width: u32,
    output_height: u32,
    coordinate_scale: f64,
    config: &TracingConfig,
) -> SvgBuildResult {
    generate_svg_with_trace_space_and_paints(
        regions,
        trace_width,
        trace_height,
        output_width,
        output_height,
        coordinate_scale,
        config,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn generate_svg_with_trace_space_and_paints(
    regions: &[ColorRegion],
    trace_width: u32,
    trace_height: u32,
    output_width: u32,
    output_height: u32,
    coordinate_scale: f64,
    config: &TracingConfig,
    gradient_paints: Option<&SvgGradientPaintMap>,
) -> SvgBuildResult {
    let mut svg = String::new();
    let mut metrics = SvgEmissionMetrics::default();
    let ordered_regions = order_regions_for_emission(regions, config);
    let ordered_region_outputs = build_ordered_region_outputs(
        &ordered_regions,
        trace_width,
        trace_height,
        output_width,
        output_height,
        coordinate_scale,
        config,
    );

    let bg = config.resolved_background_color();
    let bg_fill = format!("#{:02x}{:02x}{:02x}", bg.0, bg.1, bg.2);

    // SVG header
    svg.push_str(&format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{output_width}" height="{output_height}" viewBox="0 0 {output_width} {output_height}" shape-rendering="geometricPrecision">
"#
    ));

    // Background rectangle with configured color
    svg.push_str(&format!(
        r#"  <rect width="{output_width}" height="{output_height}" fill="{bg_fill}"/>
"#
    ));

    append_gradient_definitions(&mut svg, gradient_paints);

    // Each color region as a path group. Region path construction is parallel,
    // but final SVG serialization remains sequential to preserve deterministic output.
    for ordered_region_output in ordered_region_outputs {
        metrics.contours_suppressed_background +=
            ordered_region_output.result.contours_suppressed_background;

        for path_result in ordered_region_output.result.paths {
            metrics.contours_simplified_away += path_result.contours_simplified_away;
            metrics.contours_filtered_min_area += path_result.contours_filtered_min_area;

            if path_result.data.trim().is_empty() {
                continue;
            }

            let seam_stroke = seam_stroke_attributes(
                ordered_region_output.color,
                path_result.seam_stroke_width_centi_px,
                config,
            );
            let fill = region_fill_attributes(
                ordered_region_output.color,
                &ordered_region_output.hex,
                gradient_paints,
            );

            svg.push_str(&format!(
                r#"  <path fill="{fill}" fill-rule="evenodd"{seam_stroke} d="{path_data}"/>
"#,
                path_data = path_result.data
            ));

            metrics.regions_emitted += 1;
            metrics.contours_emitted += path_result.contours_emitted;
            metrics.holes_emitted += path_result.holes_emitted;
            metrics.points_emitted += path_result.points_emitted;
        }
    }

    svg.push_str("</svg>\n");
    SvgBuildResult { svg, metrics }
}

fn build_ordered_region_outputs(
    ordered_regions: &[OrderedRegion<'_>],
    trace_width: u32,
    trace_height: u32,
    output_width: u32,
    output_height: u32,
    coordinate_scale: f64,
    config: &TracingConfig,
) -> Vec<OrderedRegionOutput> {
    ordered_regions
        .par_iter()
        .map(|ordered_region| OrderedRegionOutput {
            color: ordered_region.region.color,
            hex: ordered_region.region.color.to_hex(),
            result: build_region_paths(
                ordered_region.region,
                trace_width,
                trace_height,
                output_width,
                output_height,
                coordinate_scale,
                config,
            ),
        })
        .collect()
}

fn order_regions_for_emission<'a>(
    regions: &'a [ColorRegion],
    config: &TracingConfig,
) -> Vec<OrderedRegion<'a>> {
    let mut ordered_regions: Vec<OrderedRegion<'a>> = regions
        .iter()
        .enumerate()
        .map(|(index, region)| OrderedRegion {
            index,
            fill_area: region_fill_area(&region.contours),
            region,
        })
        .collect();

    ordered_regions.sort_by(|left, right| {
        region_emission_priority(left.region.color, config)
            .cmp(&region_emission_priority(right.region.color, config))
            .then_with(|| {
                right
                    .fill_area
                    .total_cmp(&left.fill_area)
                    .then_with(|| left.index.cmp(&right.index))
            })
    });

    ordered_regions
}

fn region_emission_priority(color: PaletteColor, config: &TracingConfig) -> u8 {
    if is_outline_like_color(color) && !is_background_color(color, config) {
        1
    } else {
        0
    }
}

fn build_region_paths(
    region: &ColorRegion,
    trace_width: u32,
    trace_height: u32,
    output_width: u32,
    output_height: u32,
    coordinate_scale: f64,
    config: &TracingConfig,
) -> RegionBuildResult {
    let plan = region_path_groups(region, trace_width, trace_height, config);
    let paths = plan
        .contour_groups
        .into_iter()
        .map(|contour_indices| {
            build_path_data_from_indices(
                region.color,
                &region.contours,
                &contour_indices,
                output_width,
                output_height,
                coordinate_scale,
                config,
            )
        })
        .collect();

    RegionBuildResult {
        paths: merge_region_paths(paths),
        contours_suppressed_background: plan.contours_suppressed_background,
    }
}

fn merge_region_paths(paths: Vec<PathBuildResult>) -> Vec<PathBuildResult> {
    let mut merged = Vec::new();

    for mut path in paths {
        if let Some(existing) = merged.iter_mut().find(|existing: &&mut PathBuildResult| {
            existing.seam_stroke_width_centi_px == path.seam_stroke_width_centi_px
        }) {
            if !existing.data.is_empty() && !path.data.is_empty() {
                existing.data.push(' ');
            }

            existing.data.push_str(&path.data);
            existing.contours_emitted += path.contours_emitted;
            existing.holes_emitted += path.holes_emitted;
            existing.points_emitted += path.points_emitted;
            existing.contours_simplified_away += path.contours_simplified_away;
            existing.contours_filtered_min_area += path.contours_filtered_min_area;
        } else {
            path.data = path.data.trim().to_string();
            merged.push(path);
        }
    }

    merged
}

fn append_gradient_definitions(svg: &mut String, gradient_paints: Option<&SvgGradientPaintMap>) {
    let Some(gradient_paints) = gradient_paints else {
        return;
    };
    if gradient_paints.is_empty() {
        return;
    }

    let mut paints: Vec<_> = gradient_paints.values().collect();
    paints.sort_by(|left, right| left.id.cmp(&right.id));

    svg.push_str("  <defs>\n");

    for paint in paints {
        match &paint.kind {
            SvgGradientKind::Linear {
                x1,
                y1,
                x2,
                y2,
                start,
                end,
            } => {
                svg.push_str(&format!(
                    r#"    <linearGradient id="{}" gradientUnits="userSpaceOnUse" x1="{:.2}" y1="{:.2}" x2="{:.2}" y2="{:.2}">
      <stop offset="0%" stop-color="{}"/>
      <stop offset="100%" stop-color="{}"/>
    </linearGradient>
"#,
                    paint.id,
                    x1,
                    y1,
                    x2,
                    y2,
                    start.to_hex(),
                    end.to_hex(),
                ));
            }
            SvgGradientKind::Radial {
                cx,
                cy,
                radius,
                inner,
                outer,
            } => {
                svg.push_str(&format!(
                    r#"    <radialGradient id="{}" gradientUnits="userSpaceOnUse" cx="{:.2}" cy="{:.2}" r="{:.2}">
      <stop offset="0%" stop-color="{}"/>
      <stop offset="100%" stop-color="{}"/>
    </radialGradient>
"#,
                    paint.id,
                    cx,
                    cy,
                    radius,
                    inner.to_hex(),
                    outer.to_hex(),
                ));
            }
        }
    }

    svg.push_str("  </defs>\n");
}

fn region_fill_attributes(
    color: PaletteColor,
    fallback_hex: &str,
    gradient_paints: Option<&SvgGradientPaintMap>,
) -> String {
    gradient_paints
        .and_then(|gradient_paints| gradient_paints.get(&color))
        .map(|paint| format!("url(#{})", paint.id))
        .unwrap_or_else(|| fallback_hex.to_string())
}

fn region_path_groups(
    region: &ColorRegion,
    width: u32,
    height: u32,
    config: &TracingConfig,
) -> RegionPathPlan {
    if region.contours.is_empty() {
        return RegionPathPlan::default();
    }

    let hierarchy = build_contour_hierarchy(&region.contours);
    let suppressed = suppressed_background_contours(region, width, height, config);
    let mut roots = Vec::new();
    let mut root_mask = vec![false; region.contours.len()];

    for index in 0..region.contours.len() {
        if contour_is_hole(&region.contours[index]) || suppressed[index] {
            continue;
        }

        if nearest_retained_outer_ancestor(index, &hierarchy, &region.contours, &suppressed)
            .is_none()
        {
            roots.push(index);
            root_mask[index] = true;
        }
    }

    RegionPathPlan {
        contour_groups: roots
            .into_iter()
            .map(|root| {
                let mut contour_indices = Vec::new();
                collect_region_path_group(
                    root,
                    &hierarchy,
                    &suppressed,
                    &root_mask,
                    &mut contour_indices,
                );
                contour_indices
            })
            .filter(|contour_indices| !contour_indices.is_empty())
            .collect(),
        contours_suppressed_background: suppressed.iter().filter(|&&value| value).count(),
    }
}

fn build_contour_hierarchy(contours: &[Contour]) -> Vec<ContourHierarchyNode> {
    let mut hierarchy = vec![ContourHierarchyNode::default(); contours.len()];
    let bounds: Vec<ContourBounds> = contours.iter().map(contour_bounds).collect();
    let areas: Vec<f64> = contours
        .iter()
        .map(|contour| polygon_area(contour))
        .collect();
    let samples: Vec<Option<(f64, f64)>> = contours.iter().map(contour_interior_sample).collect();

    for index in 0..contours.len() {
        let Some(sample) = samples[index] else {
            continue;
        };

        let mut parent = None;
        for candidate in 0..contours.len() {
            if candidate == index || areas[candidate] <= areas[index] {
                continue;
            }
            if !bounds_contain_point(bounds[candidate], sample) {
                continue;
            }
            if !point_in_polygon(&contours[candidate], sample) {
                continue;
            }

            match parent {
                Some(existing_parent) if areas[candidate] >= areas[existing_parent] => {}
                _ => parent = Some(candidate),
            }
        }

        hierarchy[index].parent = parent;
    }

    for index in 0..hierarchy.len() {
        if let Some(parent) = hierarchy[index].parent {
            hierarchy[parent].children.push(index);
        }
    }

    hierarchy
}

fn suppressed_background_contours(
    region: &ColorRegion,
    width: u32,
    height: u32,
    config: &TracingConfig,
) -> Vec<bool> {
    if !is_background_color(region.color, config) {
        return vec![false; region.contours.len()];
    }

    region
        .contours
        .iter()
        .map(|contour| {
            !contour_is_hole(contour) && contour_touches_canvas_border(contour, width, height)
        })
        .collect()
}

fn is_background_color(color: PaletteColor, config: &TracingConfig) -> bool {
    let bg = config.resolved_background_color();
    color
        == (PaletteColor {
            r: bg.0,
            g: bg.1,
            b: bg.2,
        })
}

fn nearest_retained_outer_ancestor(
    index: usize,
    hierarchy: &[ContourHierarchyNode],
    contours: &[Contour],
    suppressed: &[bool],
) -> Option<usize> {
    let mut cursor = hierarchy[index].parent;

    while let Some(parent) = cursor {
        if !contour_is_hole(&contours[parent]) && !suppressed[parent] {
            return Some(parent);
        }

        cursor = hierarchy[parent].parent;
    }

    None
}

fn collect_region_path_group(
    index: usize,
    hierarchy: &[ContourHierarchyNode],
    suppressed: &[bool],
    root_mask: &[bool],
    contour_indices: &mut Vec<usize>,
) {
    if !suppressed[index] {
        contour_indices.push(index);
    }

    for &child in &hierarchy[index].children {
        if root_mask[child] {
            continue;
        }

        collect_region_path_group(child, hierarchy, suppressed, root_mask, contour_indices);
    }
}

fn contour_touches_canvas_border(contour: &Contour, width: u32, height: u32) -> bool {
    let max_x = width as i32;
    let max_y = height as i32;

    contour
        .iter()
        .any(|point| point.x <= 0 || point.y <= 0 || point.x >= max_x || point.y >= max_y)
}

fn contour_bounds(contour: &Contour) -> ContourBounds {
    let mut bounds = ContourBounds::default();

    if let Some(first) = contour.first() {
        bounds = ContourBounds {
            min_x: first.x,
            min_y: first.y,
            max_x: first.x,
            max_y: first.y,
        };
    }

    for point in contour.iter().skip(1) {
        bounds.min_x = bounds.min_x.min(point.x);
        bounds.min_y = bounds.min_y.min(point.y);
        bounds.max_x = bounds.max_x.max(point.x);
        bounds.max_y = bounds.max_y.max(point.y);
    }

    bounds
}

fn bounds_contain_point(bounds: ContourBounds, point: (f64, f64)) -> bool {
    point.0 >= bounds.min_x as f64
        && point.0 <= bounds.max_x as f64
        && point.1 >= bounds.min_y as f64
        && point.1 <= bounds.max_y as f64
}

fn contour_interior_sample(contour: &Contour) -> Option<(f64, f64)> {
    if contour.len() < 3 {
        return None;
    }

    let area = signed_area(contour);
    if area.abs() <= f64::EPSILON {
        return None;
    }

    for index in 0..contour.len() {
        let next_index = (index + 1) % contour.len();
        let start = contour[index];
        let end = contour[next_index];
        let dx = (end.x - start.x) as f64;
        let dy = (end.y - start.y) as f64;
        let length = (dx * dx + dy * dy).sqrt();
        if length <= f64::EPSILON {
            continue;
        }

        let (normal_x, normal_y) = if area > 0.0 {
            (-dy / length, dx / length)
        } else {
            (dy / length, -dx / length)
        };
        let sample = (
            (start.x as f64 + end.x as f64) * 0.5 + (normal_x * 0.25),
            (start.y as f64 + end.y as f64) * 0.5 + (normal_y * 0.25),
        );

        if point_in_polygon(contour, sample) {
            return Some(sample);
        }
    }

    None
}

fn point_in_polygon(contour: &Contour, point: (f64, f64)) -> bool {
    if contour.len() < 3 {
        return false;
    }

    let (px, py) = point;
    let mut inside = false;

    for index in 0..contour.len() {
        let next_index = (index + 1) % contour.len();
        let start = contour[index];
        let end = contour[next_index];
        let (x1, y1) = (start.x as f64, start.y as f64);
        let (x2, y2) = (end.x as f64, end.y as f64);

        let crosses_scanline = (y1 > py) != (y2 > py);
        if !crosses_scanline {
            continue;
        }

        let x_at_scanline = x1 + ((py - y1) * (x2 - x1) / (y2 - y1));
        if px < x_at_scanline {
            inside = !inside;
        }
    }

    inside
}

fn build_path_data_from_indices(
    region_color: PaletteColor,
    contours: &[Contour],
    contour_indices: &[usize],
    output_width: u32,
    output_height: u32,
    coordinate_scale: f64,
    config: &TracingConfig,
) -> PathBuildResult {
    let mut parts = Vec::new();
    let mut result = PathBuildResult::default();

    for &contour_index in contour_indices {
        let contour = &contours[contour_index];
        if contour.len() < 3 {
            continue;
        }

        // Simplify the closed polygon
        let simplified = simplify_closed(
            contour,
            trace_space_simplification_tolerance(config, coordinate_scale),
        );
        if simplified.len() < 3 {
            result.contours_simplified_away += 1;
            continue;
        }

        // Convert to float and apply corner-aware Laplacian smoothing
        // proportional to the configured strength.
        let float_pts: Vec<(f64, f64)> = simplified
            .iter()
            .map(|p| (p.x as f64 * coordinate_scale, p.y as f64 * coordinate_scale))
            .collect();
        let profile = contour_geometry_profile(&float_pts, coordinate_scale, config);
        let smoothed = if profile.smoothing_strength > 0.01 {
            // Derive iteration count from strength: low strength => 1 pass,
            // higher strength => more passes for stronger staircase reduction.
            let iterations = 1 + (profile.smoothing_strength * 2.0).floor() as u32;
            smooth_closed_contour_adaptive_multi(
                &float_pts,
                profile.smoothing_strength * 0.5,
                iterations,
                profile.corner_sensitivity,
            )
        } else {
            float_pts
        };

        let regularized = regularize_contour_geometry(
            &smoothed,
            config.simplification_tolerance,
            coordinate_scale,
            &profile,
            is_outline_like_color(region_color),
        );
        if regularized.len() < 3 {
            result.contours_simplified_away += 1;
            continue;
        }

        // Check minimum area using smoothed coordinates
        let area = polygon_area_f64(&regularized);
        if area < config.min_region_area {
            result.contours_filtered_min_area += 1;
            continue;
        }

        let geometry = if profile.prefer_linear || profile.smoothing_strength <= 0.01 {
            // Preserve narrow outline-like regions with straight segments.
            build_linear_path(&regularized, output_width, output_height)
        } else {
            // Use cubic Bezier curves for smoother output
            build_bezier_path(
                &regularized,
                profile.smoothing_strength,
                profile.corner_sensitivity,
                output_width,
                output_height,
            )
        };

        parts.push(geometry.data);
        result.contours_emitted += 1;
        result.points_emitted += geometry.emitted_points;
        if !contour_is_hole(&simplified) {
            result.seam_stroke_width_centi_px =
                result
                    .seam_stroke_width_centi_px
                    .max(effective_seam_stroke_width_centi_px(
                        region_color,
                        profile.seam_stroke_width_centi_px,
                    ));
        }
        if contour_is_hole(&simplified) {
            result.holes_emitted += 1;
        }
    }

    result.data = parts.join(" ");
    result
}

fn regularize_contour_geometry(
    points: &[(f64, f64)],
    simplification_tolerance: f64,
    coordinate_scale: f64,
    profile: &ContourGeometryProfile,
    outline_like: bool,
) -> Vec<(f64, f64)> {
    if points.len() < 8 || profile.smoothing_strength <= 0.01 || simplification_tolerance <= 0.0 {
        return points.to_vec();
    }

    let base_tolerance = scale_adjusted_post_smoothing_base_tolerance(
        simplification_tolerance,
        coordinate_scale,
        profile.trace_grid_tolerance_boost,
    );
    let tolerance = post_smoothing_simplification_tolerance(
        base_tolerance,
        profile.smoothing_strength,
        if outline_like {
            profile
                .post_smoothing_tolerance_factor
                .min(POST_SMOOTHING_THIN_TOLERANCE_FACTOR)
        } else {
            profile.post_smoothing_tolerance_factor
        },
    );
    if tolerance <= 0.0 {
        return points.to_vec();
    }

    let simplified = simplify_closed_f64(points, tolerance);
    if simplified.len() < 3 {
        points.to_vec()
    } else {
        simplified
    }
}

fn trace_space_simplification_tolerance(config: &TracingConfig, coordinate_scale: f64) -> f64 {
    let base = config.simplification_tolerance.max(0.0);
    if base <= 0.0 || coordinate_scale <= f64::EPSILON {
        return base;
    }

    base / coordinate_scale
}

fn contour_geometry_profile(
    points: &[(f64, f64)],
    coordinate_scale: f64,
    config: &TracingConfig,
) -> ContourGeometryProfile {
    let thickness = contour_visual_thickness(points);
    let high_detail_edge_profile = if matches!(config.quality_preset, QualityPreset::High) {
        contour_edge_profile(points)
    } else {
        ContourEdgeProfile::default()
    };
    let smoothing_factor = remap_clamped(
        thickness,
        THIN_CONTOUR_THICKNESS_PX,
        FULL_SMOOTHING_THICKNESS_PX,
        MIN_THIN_CONTOUR_SMOOTHING_FACTOR,
        1.0,
    );
    let line_like_factor = 1.0
        - remap_clamped(
            thickness,
            THIN_CONTOUR_THICKNESS_PX,
            FULL_SMOOTHING_THICKNESS_PX,
            0.0,
            1.0,
        );
    let high_detail_smoothing_factor = (1.0
        - (high_detail_edge_profile.polygonal_factor * HIGH_POLYGONAL_SMOOTHING_REDUCTION_MAX)
        - (high_detail_edge_profile.staircase_factor * HIGH_STAIRCASE_SMOOTHING_REDUCTION_MAX))
        .clamp(0.2, 1.0);
    let post_smoothing_tolerance_factor = if thickness <= LINEAR_CONTOUR_THICKNESS_PX {
        remap_clamped(
            thickness,
            THIN_CONTOUR_THICKNESS_PX,
            LINEAR_CONTOUR_THICKNESS_PX,
            POST_SMOOTHING_THIN_TOLERANCE_FACTOR,
            POST_SMOOTHING_MEDIUM_TOLERANCE_FACTOR,
        )
    } else {
        let base_factor = remap_clamped(
            thickness,
            LINEAR_CONTOUR_THICKNESS_PX,
            FULL_SMOOTHING_THICKNESS_PX,
            POST_SMOOTHING_MEDIUM_TOLERANCE_FACTOR,
            1.0,
        );
        (base_factor
            * (1.0
                - (high_detail_edge_profile.staircase_factor
                    * (1.0 - POST_SMOOTHING_STAIRCASE_TOLERANCE_FACTOR))))
            .clamp(POST_SMOOTHING_THIN_TOLERANCE_FACTOR, 1.0)
    };
    let smoothing_strength =
        config.smoothing_strength * smoothing_factor * high_detail_smoothing_factor;
    let corner_bias = (line_like_factor * 0.9)
        + (high_detail_edge_profile.polygonal_factor * 0.75)
        + (high_detail_edge_profile.staircase_factor * 0.35);
    let seam_bonus_centi_px = if matches!(config.quality_preset, QualityPreset::High) {
        HIGH_BASE_SEAM_BONUS_CENTI_PX
            + (high_detail_edge_profile.staircase_factor * HIGH_STAIRCASE_SEAM_BONUS_CENTI_PX)
    } else {
        0.0
    };
    let trace_grid_tolerance_boost = if matches!(config.quality_preset, QualityPreset::High) {
        let broad_contour_factor = remap_clamped(
            thickness,
            LINEAR_CONTOUR_THICKNESS_PX,
            FULL_SMOOTHING_THICKNESS_PX,
            0.0,
            1.0,
        );
        (broad_contour_factor
            * (HIGH_TRACE_GRID_TOLERANCE_BOOST_BASE
                + (high_detail_edge_profile.staircase_factor
                    * HIGH_TRACE_GRID_TOLERANCE_BOOST_STAIRCASE_MAX)))
            .clamp(0.0, 1.0)
    } else {
        0.0
    };

    ContourGeometryProfile {
        smoothing_strength,
        corner_sensitivity: (config.corner_sensitivity
            + ((1.0 - config.corner_sensitivity) * corner_bias.clamp(0.0, 1.0)))
        .clamp(0.0, 1.0),
        seam_stroke_width_centi_px: seam_stroke_width_centi_px(
            thickness,
            smoothing_strength,
            seam_bonus_centi_px,
            high_detail_edge_profile.staircase_factor,
            coordinate_scale,
        ),
        post_smoothing_tolerance_factor,
        trace_grid_tolerance_boost,
        prefer_linear: thickness <= LINEAR_CONTOUR_THICKNESS_PX
            || (matches!(config.quality_preset, QualityPreset::High)
                && high_detail_edge_profile.polygonal_factor >= HIGH_POLYGONAL_LINEAR_FACTOR_MIN
                && high_detail_edge_profile.point_density
                    <= HIGH_POLYGONAL_LINEAR_POINT_DENSITY_MAX),
    }
}

fn effective_seam_stroke_width_centi_px(color: PaletteColor, seam_stroke_width_centi_px: u8) -> u8 {
    if seam_stroke_width_centi_px == 0 || is_outline_like_color(color) {
        0
    } else {
        seam_stroke_width_centi_px
    }
}

fn seam_stroke_width_centi_px(
    thickness: f64,
    smoothing_strength: f64,
    seam_bonus_centi_px: f64,
    staircase_factor: f64,
    coordinate_scale: f64,
) -> u8 {
    if smoothing_strength <= 0.01 {
        return 0;
    }

    let start_thickness = MIN_SEAM_STROKE_THICKNESS_PX;

    if thickness <= start_thickness {
        return 0;
    }

    let trace_grid_scale_factor = remap_clamped(
        thickness,
        MIN_SEAM_STROKE_THICKNESS_PX,
        FULL_SEAM_STROKE_THICKNESS_PX,
        coordinate_scale.clamp(0.0, 1.0),
        1.0,
    );
    let min_seam_width = if coordinate_scale < 1.0 {
        (MIN_SEAM_STROKE_CENTI_PX * trace_grid_scale_factor).clamp(
            DENSE_TRACE_MIN_SEAM_STROKE_CENTI_PX,
            MIN_SEAM_STROKE_CENTI_PX,
        )
    } else {
        MIN_SEAM_STROKE_CENTI_PX
    };
    let max_seam_width = if coordinate_scale < 1.0 {
        (MAX_SEAM_STROKE_CENTI_PX * trace_grid_scale_factor).clamp(
            DENSE_TRACE_MAX_SEAM_STROKE_CENTI_PX,
            MAX_SEAM_STROKE_CENTI_PX,
        )
    } else {
        MAX_SEAM_STROKE_CENTI_PX
    };

    let base_width = remap_clamped(
        thickness,
        start_thickness,
        (start_thickness * 1.8).max(FULL_SEAM_STROKE_THICKNESS_PX),
        min_seam_width,
        max_seam_width,
    );
    let staircase_seam_bonus_factor = if staircase_factor <= HIGH_STAIRCASE_SEAM_GATE_FACTOR {
        1.0
    } else {
        remap_clamped(
            staircase_factor,
            HIGH_STAIRCASE_SEAM_GATE_FACTOR,
            1.0,
            1.0,
            HIGH_STAIRCASE_SEAM_BONUS_SCALE,
        )
    };
    let seam_bonus = seam_bonus_centi_px * staircase_seam_bonus_factor * trace_grid_scale_factor;
    let thin_band_penalty = if thickness < FULL_SEAM_STROKE_THICKNESS_PX {
        remap_clamped(
            thickness,
            MIN_SEAM_STROKE_THICKNESS_PX,
            FULL_SEAM_STROKE_THICKNESS_PX,
            MEDIUM_BAND_SEAM_PENALTY_MIN,
            1.0,
        )
    } else {
        1.0
    };

    let seam_width_centi_px = ((base_width + seam_bonus) * thin_band_penalty)
        .round()
        .clamp(0.0, max_seam_width) as u8;

    if coordinate_scale < 1.0 && seam_width_centi_px > 0 {
        let quantized = seam_width_centi_px
            - (seam_width_centi_px % DENSE_TRACE_SEAM_WIDTH_QUANTIZATION_STEP_CENTI_PX);
        let capped_medium_band = if thickness < FULL_SEAM_STROKE_THICKNESS_PX {
            quantized.min(DENSE_TRACE_MEDIUM_BAND_MAX_SEAM_STROKE_CENTI_PX)
        } else {
            quantized
        };

        capped_medium_band.max(DENSE_TRACE_MIN_SEAM_STROKE_CENTI_PX as u8)
    } else {
        seam_width_centi_px
    }
}

fn contour_edge_profile(points: &[(f64, f64)]) -> ContourEdgeProfile {
    let perimeter = polygon_perimeter_f64(points);
    if points.len() < 3 || perimeter <= f64::EPSILON {
        return ContourEdgeProfile::default();
    }

    let point_density = points.len() as f64 / perimeter;
    let sharp_corner_ratio = contour_sharp_corner_ratio(points);
    let polygonal_factor = sharp_corner_ratio
        * (1.0
            - remap_clamped(
                point_density,
                POLYGONAL_POINT_DENSITY_LOW,
                POLYGONAL_POINT_DENSITY_HIGH,
                0.0,
                1.0,
            ));
    let staircase_factor = sharp_corner_ratio
        * remap_clamped(
            point_density,
            STAIRCASE_POINT_DENSITY_LOW,
            STAIRCASE_POINT_DENSITY_HIGH,
            0.0,
            1.0,
        );

    ContourEdgeProfile {
        point_density,
        polygonal_factor: polygonal_factor.clamp(0.0, 1.0),
        staircase_factor: staircase_factor.clamp(0.0, 1.0),
    }
}

fn contour_sharp_corner_ratio(points: &[(f64, f64)]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }

    let mut sharp_corners = 0usize;
    let mut measured_corners = 0usize;

    for index in 0..points.len() {
        let previous = points[(index + points.len() - 1) % points.len()];
        let current = points[index];
        let next = points[(index + 1) % points.len()];

        let Some(cos_angle) = corner_cosine(previous, current, next) else {
            continue;
        };

        measured_corners += 1;
        if cos_angle <= SHARP_CORNER_COS_THRESHOLD {
            sharp_corners += 1;
        }
    }

    if measured_corners == 0 {
        0.0
    } else {
        sharp_corners as f64 / measured_corners as f64
    }
}

fn seam_stroke_attributes(
    color: PaletteColor,
    seam_stroke_width_centi_px: u8,
    config: &TracingConfig,
) -> String {
    if effective_seam_stroke_width_centi_px(color, seam_stroke_width_centi_px) == 0
        || is_background_color(color, config)
    {
        return String::from(r#" stroke="none""#);
    }

    format!(
        r#" stroke="{}" stroke-width="{:.2}" stroke-linejoin="round" paint-order="stroke fill" vector-effect="non-scaling-stroke""#,
        color.to_hex(),
        effective_seam_stroke_width_centi_px(color, seam_stroke_width_centi_px) as f64 / 100.0,
    )
}

fn is_outline_like_color(color: PaletteColor) -> bool {
    let max_channel = color.r.max(color.g).max(color.b);
    let min_channel = color.r.min(color.g).min(color.b);
    color_luminance(color) <= OUTLINE_COLOR_LUMA_MAX
        && max_channel.saturating_sub(min_channel) <= OUTLINE_COLOR_CHROMA_MAX
}

fn color_luminance(color: PaletteColor) -> f64 {
    (0.2126 * f64::from(color.r)) + (0.7152 * f64::from(color.g)) + (0.0722 * f64::from(color.b))
}

fn post_smoothing_simplification_tolerance(
    simplification_tolerance: f64,
    smoothing_strength: f64,
    tolerance_factor: f64,
) -> f64 {
    let base = simplification_tolerance.max(0.0);
    if base <= 0.0 {
        return 0.0;
    }

    (base * tolerance_factor.clamp(0.0, 1.0) * (0.75 + (smoothing_strength * 0.50))).max(0.08)
}

fn scale_adjusted_post_smoothing_base_tolerance(
    simplification_tolerance: f64,
    coordinate_scale: f64,
    trace_grid_tolerance_boost: f64,
) -> f64 {
    let base = simplification_tolerance.max(0.0);
    if base <= 0.0
        || coordinate_scale <= f64::EPSILON
        || coordinate_scale >= 1.0
        || trace_grid_tolerance_boost <= 0.0
    {
        return base;
    }

    let trace_grid_base = base / coordinate_scale;
    base + ((trace_grid_base - base) * trace_grid_tolerance_boost.clamp(0.0, 1.0))
}

fn contour_visual_thickness(points: &[(f64, f64)]) -> f64 {
    let perimeter = polygon_perimeter_f64(points);
    if perimeter <= f64::EPSILON {
        return 0.0;
    }

    (polygon_area_f64(points) * 2.0) / perimeter
}

fn polygon_perimeter_f64(points: &[(f64, f64)]) -> f64 {
    let n = points.len();
    if n < 2 {
        return 0.0;
    }

    let mut perimeter = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        let dx = points[j].0 - points[i].0;
        let dy = points[j].1 - points[i].1;
        perimeter += (dx * dx + dy * dy).sqrt();
    }

    perimeter
}

fn remap_clamped(value: f64, in_min: f64, in_max: f64, out_min: f64, out_max: f64) -> f64 {
    if in_max <= in_min {
        return out_max;
    }

    let t = ((value - in_min) / (in_max - in_min)).clamp(0.0, 1.0);
    out_min + ((out_max - out_min) * t)
}

fn region_fill_area(contours: &[Contour]) -> f64 {
    contours
        .iter()
        .map(|contour| {
            let area = polygon_area(contour);
            if contour_is_hole(contour) {
                -area
            } else {
                area
            }
        })
        .sum::<f64>()
        .max(0.0)
}

fn format_svg_point(x: f64, y: f64) -> String {
    format!(
        "{x:.precision$} {y:.precision$}",
        precision = SVG_COORD_PRECISION
    )
}

fn clamp_svg_point(point: (f64, f64), width: u32, height: u32) -> (f64, f64) {
    (
        point.0.clamp(0.0, width as f64),
        point.1.clamp(0.0, height as f64),
    )
}

fn clamp_bezier_to_canvas(bezier: CubicBezier, width: u32, height: u32) -> CubicBezier {
    CubicBezier {
        p0: clamp_svg_point(bezier.p0, width, height),
        p1: clamp_svg_point(bezier.p1, width, height),
        p2: clamp_svg_point(bezier.p2, width, height),
        p3: clamp_svg_point(bezier.p3, width, height),
    }
}

/// Build a path using straight line segments.
fn build_linear_path(points: &[(f64, f64)], width: u32, height: u32) -> PathGeometry {
    let mut d = String::new();
    for (i, &p) in points.iter().enumerate() {
        if i == 0 {
            d.push_str("M ");
        } else {
            d.push_str(" L ");
        }

        let point = clamp_svg_point(p, width, height);
        d.push_str(&format_svg_point(point.0, point.1));
    }
    d.push_str(" Z");
    PathGeometry {
        data: d,
        emitted_points: points.len(),
    }
}

/// Build a path using cubic Bezier curves.
fn build_bezier_path(
    points: &[(f64, f64)],
    smoothing: f64,
    corner_sensitivity: f64,
    width: u32,
    height: u32,
) -> PathGeometry {
    let beziers: Vec<CubicBezier> =
        fit_closed_cubic_beziers_f64(points, smoothing, corner_sensitivity)
            .into_iter()
            .map(|bezier| clamp_bezier_to_canvas(bezier, width, height))
            .collect();
    if beziers.is_empty() {
        return build_linear_path(points, width, height);
    }

    let mut d = String::new();
    d.push_str("M ");
    d.push_str(&format_svg_point(beziers[0].p0.0, beziers[0].p0.1));

    for bez in &beziers {
        d.push_str(" C ");
        d.push_str(&format_svg_point(bez.p1.0, bez.p1.1));
        d.push_str(", ");
        d.push_str(&format_svg_point(bez.p2.0, bez.p2.1));
        d.push_str(", ");
        d.push_str(&format_svg_point(bez.p3.0, bez.p3.1));
    }
    d.push_str(" Z");
    PathGeometry {
        data: d,
        // One coordinate pair is emitted by the initial `M`, and each cubic
        // `C` segment contributes three more coordinate pairs.
        emitted_points: 1 + (beziers.len() * 3),
    }
}

/// Calculate the signed area of a polygon using the shoelace formula.
fn polygon_area(points: &[Point]) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut area = 0.0f64;
    for i in 0..n {
        let j = (i + 1) % n;
        area += points[i].x as f64 * points[j].y as f64;
        area -= points[j].x as f64 * points[i].y as f64;
    }
    (area / 2.0).abs()
}

/// Calculate the area of a polygon from float coordinates.
fn polygon_area_f64(points: &[(f64, f64)]) -> f64 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut area = 0.0f64;
    for i in 0..n {
        let j = (i + 1) % n;
        area += points[i].0 * points[j].1;
        area -= points[j].0 * points[i].1;
    }
    (area / 2.0).abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contour::Point;

    fn assert_two_decimal_precision(path_data: &str) {
        let decimals: Vec<&str> = path_data
            .split(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '-'))
            .filter(|token| token.contains('.'))
            .collect();

        assert!(
            !decimals.is_empty(),
            "expected decimal coordinate tokens in {path_data}"
        );
        assert!(
            decimals.iter().all(|token| token
                .split_once('.')
                .is_some_and(|(_, fraction)| fraction.len() == SVG_COORD_PRECISION)),
            "all SVG coordinate tokens must use exactly {SVG_COORD_PRECISION} decimal places: {path_data}"
        );
    }

    fn extract_path_numbers(path_data: &str) -> Vec<f64> {
        path_data
            .split(|ch: char| !(ch.is_ascii_digit() || ch == '.' || ch == '-'))
            .filter_map(|token| token.parse::<f64>().ok())
            .collect()
    }

    #[test]
    fn polygon_area_square() {
        let pts = vec![
            Point::new(0, 0),
            Point::new(10, 0),
            Point::new(10, 10),
            Point::new(0, 10),
        ];
        let area = polygon_area(&pts);
        assert!((area - 100.0).abs() < 1e-6);
    }

    #[test]
    fn generate_svg_produces_valid_header() {
        let config = crate::config::TracingConfig::default();
        let regions = vec![];
        let svg = generate_svg(&regions, 100, 100, &config);
        assert!(svg.contains(r#"<svg xmlns="http://www.w3.org/2000/svg""#));
        assert!(svg.contains(r#"viewBox="0 0 100 100""#));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn generate_svg_uses_evenodd_fill_rule() {
        let config = crate::config::TracingConfig::default();
        let regions = vec![ColorRegion {
            color: PaletteColor { r: 0, g: 0, b: 0 },
            contours: vec![
                vec![
                    Point::new(0, 0),
                    Point::new(4, 0),
                    Point::new(4, 4),
                    Point::new(0, 4),
                ],
                vec![
                    Point::new(1, 1),
                    Point::new(1, 3),
                    Point::new(3, 3),
                    Point::new(3, 1),
                ],
            ],
        }];

        let svg = generate_svg(&regions, 4, 4, &config);
        assert!(svg.contains(r#"fill-rule="evenodd""#));
    }

    #[test]
    fn generate_svg_with_metrics_counts_emitted_geometry() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.0,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let regions = vec![ColorRegion {
            color: PaletteColor { r: 0, g: 0, b: 0 },
            contours: vec![
                vec![
                    Point::new(0, 0),
                    Point::new(4, 0),
                    Point::new(4, 4),
                    Point::new(0, 4),
                ],
                vec![
                    Point::new(1, 1),
                    Point::new(1, 3),
                    Point::new(3, 3),
                    Point::new(3, 1),
                ],
            ],
        }];

        let result = generate_svg_with_metrics(&regions, 4, 4, &config);
        assert_eq!(result.metrics.regions_emitted, 1);
        assert_eq!(result.metrics.contours_emitted, 2);
        assert_eq!(result.metrics.holes_emitted, 1);
        assert_eq!(result.metrics.points_emitted, 8);
    }

    #[test]
    fn generate_svg_merges_same_color_disconnected_paths() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.0,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let regions = vec![ColorRegion {
            color: PaletteColor { r: 0, g: 0, b: 0 },
            contours: vec![
                vec![
                    Point::new(0, 0),
                    Point::new(4, 0),
                    Point::new(4, 4),
                    Point::new(0, 4),
                ],
                vec![
                    Point::new(6, 0),
                    Point::new(10, 0),
                    Point::new(10, 4),
                    Point::new(6, 4),
                ],
            ],
        }];

        let result = generate_svg_with_metrics(&regions, 10, 4, &config);

        assert_eq!(result.metrics.regions_emitted, 1);
        assert_eq!(result.metrics.contours_emitted, 2);
        assert_eq!(result.svg.matches("<path").count(), 1);
        assert!(result.svg.matches("M ").count() >= 2);
    }

    #[test]
    fn generate_svg_with_gradient_paints_emits_defs_and_url_fill() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.0,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let color = PaletteColor {
            r: 128,
            g: 32,
            b: 128,
        };
        let mut gradient_paints = SvgGradientPaintMap::new();
        gradient_paints.insert(
            color,
            crate::pipeline::gradient::SvgGradientPaint {
                id: "grad-802080".to_string(),
                kind: SvgGradientKind::Linear {
                    x1: 0.0,
                    y1: 2.0,
                    x2: 8.0,
                    y2: 2.0,
                    start: PaletteColor {
                        r: 32,
                        g: 32,
                        b: 192,
                    },
                    end: PaletteColor {
                        r: 224,
                        g: 32,
                        b: 64,
                    },
                },
            },
        );
        let regions = vec![ColorRegion {
            color,
            contours: vec![vec![
                Point::new(0, 0),
                Point::new(8, 0),
                Point::new(8, 4),
                Point::new(0, 4),
            ]],
        }];

        let result =
            generate_svg_with_metrics_and_paints(&regions, 8, 4, &config, Some(&gradient_paints));

        assert!(result.svg.contains("<defs>"));
        assert!(result.svg.contains("<linearGradient"));
        assert!(result.svg.contains(r##"fill="url(#grad-802080)""##));
    }

    #[test]
    fn generate_svg_with_metrics_counts_bezier_control_points() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.6,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let regions = vec![ColorRegion {
            color: PaletteColor { r: 0, g: 0, b: 0 },
            contours: vec![vec![
                Point::new(0, 0),
                Point::new(20, 0),
                Point::new(20, 20),
                Point::new(0, 20),
            ]],
        }];

        let result = generate_svg_with_metrics(&regions, 20, 20, &config);
        assert!(result.svg.contains(" C "));
        assert_eq!(result.svg.matches(" C ").count(), 4);
        assert_eq!(result.metrics.regions_emitted, 1);
        assert_eq!(result.metrics.contours_emitted, 1);
        assert_eq!(result.metrics.points_emitted, 13);
    }

    #[test]
    fn generate_svg_with_trace_space_maps_internal_coordinates_back_to_output_space() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.0,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let regions = vec![ColorRegion {
            color: PaletteColor { r: 0, g: 0, b: 0 },
            contours: vec![vec![
                Point::new(0, 0),
                Point::new(5, 0),
                Point::new(5, 5),
                Point::new(0, 5),
            ]],
        }];

        let result = generate_svg_with_trace_space(&regions, 6, 6, 3, 3, 0.5, &config);

        assert!(result.svg.contains(r#"viewBox="0 0 3 3""#));
        assert!(result
            .svg
            .contains("M 0.00 0.00 L 2.50 0.00 L 2.50 2.50 L 0.00 2.50 Z"));
    }

    #[test]
    fn generate_svg_emits_larger_regions_before_smaller_details() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.0,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let regions = vec![
            ColorRegion {
                color: PaletteColor { r: 255, g: 0, b: 0 },
                contours: vec![vec![
                    Point::new(2, 2),
                    Point::new(4, 2),
                    Point::new(4, 4),
                    Point::new(2, 4),
                ]],
            },
            ColorRegion {
                color: PaletteColor { r: 0, g: 0, b: 255 },
                contours: vec![vec![
                    Point::new(0, 0),
                    Point::new(8, 0),
                    Point::new(8, 8),
                    Point::new(0, 8),
                ]],
            },
        ];

        let svg = generate_svg(&regions, 8, 8, &config);
        let background_index = svg.find("fill=\"#0000ff\"").unwrap();
        let detail_index = svg.find("fill=\"#ff0000\"").unwrap();

        assert!(background_index < detail_index);
    }

    #[test]
    fn generate_svg_emits_outline_like_regions_after_colored_fills() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.4,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let regions = vec![
            ColorRegion {
                color: PaletteColor { r: 6, g: 6, b: 3 },
                contours: vec![vec![
                    Point::new(0, 0),
                    Point::new(12, 0),
                    Point::new(12, 12),
                    Point::new(0, 12),
                ]],
            },
            ColorRegion {
                color: PaletteColor { r: 0, g: 0, b: 255 },
                contours: vec![vec![
                    Point::new(2, 2),
                    Point::new(10, 2),
                    Point::new(10, 10),
                    Point::new(2, 10),
                ]],
            },
        ];

        let svg = generate_svg(&regions, 12, 12, &config);
        let fill_index = svg.find(r##"fill="#0000ff""##).unwrap();
        let outline_index = svg.rfind(r##"fill="#060603""##).unwrap();

        assert!(fill_index < outline_index);
    }

    #[test]
    fn build_ordered_region_outputs_preserves_emission_order() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.0,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let regions = vec![
            ColorRegion {
                color: PaletteColor { r: 255, g: 0, b: 0 },
                contours: vec![vec![
                    Point::new(2, 2),
                    Point::new(4, 2),
                    Point::new(4, 4),
                    Point::new(2, 4),
                ]],
            },
            ColorRegion {
                color: PaletteColor { r: 0, g: 0, b: 255 },
                contours: vec![vec![
                    Point::new(0, 0),
                    Point::new(8, 0),
                    Point::new(8, 8),
                    Point::new(0, 8),
                ]],
            },
            ColorRegion {
                color: PaletteColor { r: 6, g: 6, b: 3 },
                contours: vec![vec![
                    Point::new(1, 1),
                    Point::new(7, 1),
                    Point::new(7, 7),
                    Point::new(1, 7),
                ]],
            },
        ];

        let ordered_regions = order_regions_for_emission(&regions, &config);
        let ordered_colors: Vec<PaletteColor> = ordered_regions
            .iter()
            .map(|ordered_region| ordered_region.region.color)
            .collect();

        let outputs = build_ordered_region_outputs(&ordered_regions, 8, 8, 8, 8, 1.0, &config);
        let output_colors: Vec<PaletteColor> = outputs
            .iter()
            .map(|ordered_region_output| ordered_region_output.color)
            .collect();

        assert_eq!(output_colors, ordered_colors);
    }

    #[test]
    fn generate_svg_skips_redundant_white_background_region() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.0,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let regions = vec![
            ColorRegion {
                color: PaletteColor {
                    r: 255,
                    g: 255,
                    b: 255,
                },
                contours: vec![vec![
                    Point::new(0, 0),
                    Point::new(8, 0),
                    Point::new(8, 8),
                    Point::new(0, 8),
                ]],
            },
            ColorRegion {
                color: PaletteColor { r: 0, g: 0, b: 0 },
                contours: vec![vec![
                    Point::new(2, 2),
                    Point::new(6, 2),
                    Point::new(6, 6),
                    Point::new(2, 6),
                ]],
            },
        ];

        let svg = generate_svg(&regions, 8, 8, &config);

        // Background rect should use the configured color (white by default)
        assert!(svg.contains("fill=\"#ffffff\""));
        assert!(svg.contains("fill=\"#000000\""));
        // The redundant white contour touching the border should be suppressed,
        // so only 1 <path> (the black region) should be emitted.
        assert_eq!(svg.matches("<path").count(), 1);
    }

    #[test]
    fn generate_svg_keeps_interior_white_region() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.0,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let regions = vec![
            ColorRegion {
                color: PaletteColor { r: 0, g: 0, b: 0 },
                contours: vec![vec![
                    Point::new(0, 0),
                    Point::new(8, 0),
                    Point::new(8, 8),
                    Point::new(0, 8),
                ]],
            },
            ColorRegion {
                color: PaletteColor {
                    r: 255,
                    g: 255,
                    b: 255,
                },
                contours: vec![vec![
                    Point::new(2, 2),
                    Point::new(6, 2),
                    Point::new(6, 6),
                    Point::new(2, 6),
                ]],
            },
        ];

        let svg = generate_svg(&regions, 8, 8, &config);

        assert!(svg.contains("fill=\"#000000\""));
        assert!(svg.contains("fill=\"#ffffff\""));
        assert_eq!(svg.matches("<path").count(), 2);
    }

    #[test]
    fn generate_svg_keeps_nested_white_island_after_background_suppression() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.0,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let regions = vec![ColorRegion {
            color: PaletteColor {
                r: 255,
                g: 255,
                b: 255,
            },
            contours: vec![
                vec![
                    Point::new(0, 0),
                    Point::new(8, 0),
                    Point::new(8, 8),
                    Point::new(0, 8),
                ],
                vec![
                    Point::new(2, 2),
                    Point::new(2, 6),
                    Point::new(6, 6),
                    Point::new(6, 2),
                ],
                vec![
                    Point::new(3, 3),
                    Point::new(5, 3),
                    Point::new(5, 5),
                    Point::new(3, 5),
                ],
            ],
        }];

        let result = generate_svg_with_metrics(&regions, 8, 8, &config);

        assert!(result.svg.contains("fill=\"#ffffff\""));
        assert!(result.svg.contains("M 3.00 3.00"));
        assert!(!result
            .svg
            .contains("M 0.00 0.00 L 8.00 0.00 L 8.00 8.00 L 0.00 8.00 Z"));
        assert_eq!(result.metrics.regions_emitted, 1);
        assert_eq!(result.metrics.contours_emitted, 1);
    }

    #[test]
    fn build_linear_path_uses_two_decimal_precision() {
        let geometry = build_linear_path(&[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0)], 4, 4);

        assert_two_decimal_precision(&geometry.data);
        assert!(geometry.data.contains("M 0.00 0.00"));
        assert!(geometry.data.contains("L 4.00 0.00"));
    }

    #[test]
    fn build_bezier_path_uses_two_decimal_precision() {
        let geometry = build_bezier_path(
            &[(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)],
            0.6,
            0.6,
            4,
            4,
        );

        assert_two_decimal_precision(&geometry.data);
        assert!(geometry.data.contains("M 0.00 0.00"));
        assert!(geometry.data.contains(" C "));
        assert!(geometry.data.contains("4.00 0.00"));
    }

    #[test]
    fn build_bezier_path_clamps_edge_touching_curves_to_canvas_bounds() {
        let geometry = build_bezier_path(
            &[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)],
            0.8,
            0.0,
            20,
            20,
        );

        let coordinates = extract_path_numbers(&geometry.data);

        assert!(geometry.data.contains(" C "));
        assert!(!coordinates.is_empty());
        assert!(coordinates.iter().all(|value| (0.0..=20.0).contains(value)));
    }

    #[test]
    fn build_path_data_counts_simplified_away_contours() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.0,
            simplification_tolerance: 10.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let contours = vec![vec![
            Point::new(1, 1),
            Point::new(1, 1),
            Point::new(1, 1),
            Point::new(1, 1),
        ]];

        let result = build_path_data_from_indices(
            PaletteColor { r: 0, g: 0, b: 0 },
            &contours,
            &[0],
            4,
            4,
            1.0,
            &config,
        );

        assert!(result.data.is_empty());
        assert_eq!(result.contours_emitted, 0);
        assert_eq!(result.contours_simplified_away, 1);
        assert_eq!(result.contours_filtered_min_area, 0);
    }

    #[test]
    fn regularize_contour_geometry_keeps_small_simple_shapes() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.6,
            simplification_tolerance: 1.0,
            ..crate::config::TracingConfig::default()
        };
        let points = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];
        let profile = ContourGeometryProfile {
            smoothing_strength: config.smoothing_strength,
            post_smoothing_tolerance_factor: 1.0,
            ..ContourGeometryProfile::default()
        };

        let regularized = regularize_contour_geometry(
            &points,
            config.simplification_tolerance,
            1.0,
            &profile,
            false,
        );

        assert_eq!(regularized, points);
    }

    #[test]
    fn regularize_contour_geometry_reduces_tiny_outline_wiggles() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.6,
            simplification_tolerance: 1.0,
            ..crate::config::TracingConfig::default()
        };
        let points = vec![
            (0.0, 0.0),
            (1.0, 0.03),
            (2.0, -0.02),
            (3.0, 0.04),
            (4.0, 0.0),
            (5.0, 0.02),
            (6.0, -0.03),
            (7.0, 0.0),
            (7.0, 4.0),
            (0.0, 4.0),
        ];
        let profile = ContourGeometryProfile {
            smoothing_strength: config.smoothing_strength,
            post_smoothing_tolerance_factor: 1.0,
            ..ContourGeometryProfile::default()
        };

        let regularized = regularize_contour_geometry(
            &points,
            config.simplification_tolerance,
            1.0,
            &profile,
            false,
        );

        assert!(regularized.len() < points.len());
        assert!(regularized.len() >= 4);
    }

    #[test]
    fn trace_space_simplification_tolerance_scales_with_dense_trace_grids() {
        let config = crate::config::TracingConfig {
            simplification_tolerance: 1.2,
            ..crate::config::TracingConfig::default()
        };

        assert!((trace_space_simplification_tolerance(&config, 1.0) - 1.2).abs() < 1e-10);
        assert!((trace_space_simplification_tolerance(&config, 0.5) - 2.4).abs() < 1e-10);
    }

    #[test]
    fn post_smoothing_regularization_tolerance_stays_in_output_space() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.35,
            simplification_tolerance: 1.2,
            ..crate::config::TracingConfig::default()
        };

        let tolerance = post_smoothing_simplification_tolerance(
            config.simplification_tolerance,
            config.smoothing_strength,
            1.0,
        );
        assert!(tolerance > 1.0);
        assert!(tolerance < 1.2);
    }

    #[test]
    fn scale_adjusted_post_smoothing_base_tolerance_only_boosts_dense_trace_grids() {
        assert!((scale_adjusted_post_smoothing_base_tolerance(1.0, 1.0, 0.8) - 1.0).abs() < 1e-10);
        assert!((scale_adjusted_post_smoothing_base_tolerance(1.0, 0.5, 0.0) - 1.0).abs() < 1e-10);
        assert!((scale_adjusted_post_smoothing_base_tolerance(1.0, 0.5, 0.5) - 1.5).abs() < 1e-10);
    }

    #[test]
    fn staircase_seam_gate_does_not_require_extra_thickness() {
        assert_eq!(seam_stroke_width_centi_px(4.0, 0.5, 6.0, 0.8, 1.0), 0);
        assert!(seam_stroke_width_centi_px(6.0, 0.5, 6.0, 0.0, 1.0) > 0);
        assert!(seam_stroke_width_centi_px(6.0, 0.5, 6.0, 0.8, 1.0) > 0);
    }

    #[test]
    fn dense_trace_grids_scale_down_medium_band_seams_without_shrinking_broad_fills() {
        let medium_default = seam_stroke_width_centi_px(6.0, 0.4, 6.0, 0.0, 1.0);
        let medium_dense = seam_stroke_width_centi_px(6.0, 0.4, 6.0, 0.0, 0.5);
        let medium_broadish_dense = seam_stroke_width_centi_px(10.0, 0.4, 10.0, 0.0, 0.5);
        let broad_default = seam_stroke_width_centi_px(20.0, 0.4, 6.0, 0.0, 1.0);
        let broad_dense = seam_stroke_width_centi_px(20.0, 0.4, 6.0, 0.0, 0.5);

        assert!(medium_dense < medium_default);
        assert!(medium_broadish_dense <= DENSE_TRACE_MEDIUM_BAND_MAX_SEAM_STROKE_CENTI_PX);
        assert_eq!(broad_dense, broad_default);
    }

    #[test]
    fn staircase_seam_bonus_tapers_gradually_above_the_gate() {
        let near_gate = seam_stroke_width_centi_px(
            10.0,
            0.4,
            10.0,
            HIGH_STAIRCASE_SEAM_GATE_FACTOR + 0.01,
            1.0,
        );
        let extreme = seam_stroke_width_centi_px(10.0, 0.4, 10.0, 1.0, 1.0);
        let no_bonus = seam_stroke_width_centi_px(10.0, 0.4, 0.0, 1.0, 1.0);

        assert!(near_gate > extreme);
        assert!(extreme > no_bonus);
    }

    #[test]
    fn dense_trace_grids_quantize_seam_widths_to_even_centi_pixel_steps() {
        let dense_trace_seam = seam_stroke_width_centi_px(7.6, 0.4, 6.0, 0.2, 0.5);

        assert_eq!(
            dense_trace_seam % DENSE_TRACE_SEAM_WIDTH_QUANTIZATION_STEP_CENTI_PX,
            0
        );
        assert!(dense_trace_seam >= DENSE_TRACE_MIN_SEAM_STROKE_CENTI_PX as u8);
    }

    #[test]
    fn contour_geometry_profile_reduces_smoothing_for_thin_regions() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.5,
            corner_sensitivity: 0.6,
            ..crate::config::TracingConfig::default()
        };
        let thin = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 2.0), (0.0, 2.0)];
        let broad = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];

        let thin_profile = contour_geometry_profile(&thin, 1.0, &config);
        let broad_profile = contour_geometry_profile(&broad, 1.0, &config);

        assert!(thin_profile.smoothing_strength < broad_profile.smoothing_strength);
        assert!(thin_profile.corner_sensitivity > broad_profile.corner_sensitivity);
        assert!(
            thin_profile.post_smoothing_tolerance_factor
                < broad_profile.post_smoothing_tolerance_factor
        );
        assert!(thin_profile.prefer_linear);
        assert!(!broad_profile.prefer_linear);
        assert!(
            thin_profile.trace_grid_tolerance_boost <= broad_profile.trace_grid_tolerance_boost
        );
    }

    #[test]
    fn high_preset_polygonal_regions_reduce_smoothing_and_prefer_linear() {
        let config = crate::config::QualityPreset::High.to_config();
        let polygonal = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
        let organic = vec![
            (0.0, 10.0),
            (4.0, 2.0),
            (12.0, 0.0),
            (19.0, 4.0),
            (20.0, 12.0),
            (16.0, 19.0),
            (8.0, 20.0),
            (1.0, 16.0),
        ];

        let polygonal_profile = contour_geometry_profile(&polygonal, 1.0, &config);
        let organic_profile = contour_geometry_profile(&organic, 1.0, &config);

        assert!(polygonal_profile.smoothing_strength < organic_profile.smoothing_strength);
        assert!(polygonal_profile.corner_sensitivity > organic_profile.corner_sensitivity);
        assert!(polygonal_profile.prefer_linear);
        assert!(!organic_profile.prefer_linear);
    }

    #[test]
    fn high_preset_applies_broader_seam_closing_than_balanced() {
        let balanced = crate::config::TracingConfig {
            smoothing_strength: 0.35,
            ..crate::config::TracingConfig::default()
        };
        let high = crate::config::QualityPreset::High.to_config();
        let broad = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];

        let balanced_profile = contour_geometry_profile(&broad, 1.0, &balanced);
        let high_profile = contour_geometry_profile(&broad, 1.0, &high);

        assert!(
            high_profile.seam_stroke_width_centi_px > balanced_profile.seam_stroke_width_centi_px
        );
    }

    #[test]
    fn seam_stroke_width_skips_thin_regions() {
        assert_eq!(seam_stroke_width_centi_px(2.0, 0.4, 0.0, 0.0, 1.0), 0);
        assert!(seam_stroke_width_centi_px(10.0, 0.4, 0.0, 0.0, 1.0) > 0);
        assert!(seam_stroke_width_centi_px(10.0, 0.4, 10.0, 0.8, 1.0) > 0);
        assert!(
            seam_stroke_width_centi_px(10.0, 0.4, 10.0, 0.8, 1.0)
                > seam_stroke_width_centi_px(10.0, 0.4, 0.0, 0.8, 1.0)
        );
        assert_eq!(
            seam_stroke_width_centi_px(40.0, 0.8, 40.0, 0.0, 1.0),
            MAX_SEAM_STROKE_CENTI_PX as u8
        );
    }

    #[test]
    fn outline_like_colors_skip_seam_strokes() {
        let outline = PaletteColor { r: 6, g: 6, b: 3 };
        let accent = PaletteColor { r: 5, g: 158, b: 8 };

        assert!(is_outline_like_color(outline));
        assert!(!is_outline_like_color(accent));
        assert_eq!(effective_seam_stroke_width_centi_px(outline, 20), 0);
        assert_eq!(effective_seam_stroke_width_centi_px(accent, 20), 20);
    }

    #[test]
    fn generate_svg_adds_fill_colored_seam_stroke_for_broad_regions() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.4,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let regions = vec![ColorRegion {
            color: PaletteColor { r: 255, g: 0, b: 0 },
            contours: vec![vec![
                Point::new(0, 0),
                Point::new(12, 0),
                Point::new(12, 12),
                Point::new(0, 12),
            ]],
        }];

        let svg = generate_svg(&regions, 12, 12, &config);
        assert!(svg.contains(r##"stroke="#ff0000""##));
        assert!(svg.contains(r##"paint-order="stroke fill""##));
    }

    #[test]
    fn generate_svg_does_not_stroke_outline_like_regions() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.4,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let regions = vec![ColorRegion {
            color: PaletteColor { r: 6, g: 6, b: 3 },
            contours: vec![vec![
                Point::new(0, 0),
                Point::new(12, 0),
                Point::new(12, 12),
                Point::new(0, 12),
            ]],
        }];

        let svg = generate_svg(&regions, 12, 12, &config);
        assert!(svg.contains(r##"fill="#060603" fill-rule="evenodd" stroke="none""##));
    }

    #[test]
    fn generate_svg_skips_seam_stroke_for_background_and_thin_regions() {
        let config = crate::config::TracingConfig {
            smoothing_strength: 0.4,
            simplification_tolerance: 0.0,
            min_region_area: 0.0,
            ..crate::config::TracingConfig::default()
        };
        let regions = vec![
            ColorRegion {
                color: PaletteColor {
                    r: 255,
                    g: 255,
                    b: 255,
                },
                contours: vec![vec![
                    Point::new(0, 0),
                    Point::new(12, 0),
                    Point::new(12, 12),
                    Point::new(0, 12),
                ]],
            },
            ColorRegion {
                color: PaletteColor { r: 0, g: 0, b: 0 },
                contours: vec![vec![
                    Point::new(0, 0),
                    Point::new(20, 0),
                    Point::new(20, 2),
                    Point::new(0, 2),
                ]],
            },
        ];

        let svg = generate_svg(&regions, 20, 12, &config);
        assert_eq!(
            seam_stroke_attributes(
                PaletteColor {
                    r: 255,
                    g: 255,
                    b: 255,
                },
                20,
                &config,
            ),
            r#" stroke="none""#
        );
        assert!(svg.contains(r##"fill="#000000" fill-rule="evenodd" stroke="none""##));
    }
}
