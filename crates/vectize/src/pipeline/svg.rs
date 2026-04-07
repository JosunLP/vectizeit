//! SVG generation from traced contours and Bezier paths.
//!
//! Produces valid, clean SVG markup with proper viewBox, hole-preserving path
//! elements, deterministic output, and fixed coordinate precision across
//! linear and Bezier path serialization.

use crate::config::TracingConfig;
use crate::pipeline::contour::{contour_is_hole, signed_area, Contour, Point};
use crate::pipeline::curves::{fit_closed_cubic_beziers_f64, CubicBezier};
use crate::pipeline::segment::PaletteColor;
use crate::pipeline::simplify::simplify_closed;
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

#[derive(Clone, Copy)]
struct OrderedRegion<'a> {
    index: usize,
    fill_area: f64,
    region: &'a ColorRegion,
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
    let mut svg = String::new();
    let mut metrics = SvgEmissionMetrics::default();
    let ordered_regions = order_regions_for_emission(regions);

    let bg = config.background_color.unwrap_or((255, 255, 255));
    let bg_fill = format!("#{:02x}{:02x}{:02x}", bg.0, bg.1, bg.2);

    // SVG header
    svg.push_str(&format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
"#
    ));

    // Background rectangle with configured color
    svg.push_str(&format!(
        r#"  <rect width="{width}" height="{height}" fill="{bg_fill}"/>
"#
    ));

    // Each color region as a path group
    for ordered_region in ordered_regions {
        let region = ordered_region.region;

        let hex = region.color.to_hex();

        let region_result = build_region_paths(region, width, height, config);
        metrics.contours_suppressed_background += region_result.contours_suppressed_background;

        for path_result in region_result.paths {
            metrics.contours_simplified_away += path_result.contours_simplified_away;
            metrics.contours_filtered_min_area += path_result.contours_filtered_min_area;

            if path_result.data.trim().is_empty() {
                continue;
            }

            svg.push_str(&format!(
                r#"  <path fill="{hex}" fill-rule="evenodd" stroke="none" d="{path_data}"/>
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

fn order_regions_for_emission(regions: &[ColorRegion]) -> Vec<OrderedRegion<'_>> {
    let mut ordered_regions: Vec<OrderedRegion<'_>> = regions
        .iter()
        .enumerate()
        .map(|(index, region)| OrderedRegion {
            index,
            fill_area: region_fill_area(&region.contours),
            region,
        })
        .collect();

    ordered_regions.sort_by(|left, right| {
        right
            .fill_area
            .total_cmp(&left.fill_area)
            .then_with(|| left.index.cmp(&right.index))
    });

    ordered_regions
}

fn build_region_paths(
    region: &ColorRegion,
    width: u32,
    height: u32,
    config: &TracingConfig,
) -> RegionBuildResult {
    let plan = region_path_groups(region, width, height, config);

    RegionBuildResult {
        paths: plan
            .contour_groups
            .into_iter()
            .map(|contour_indices| {
                build_path_data_from_indices(
                    &region.contours,
                    &contour_indices,
                    width,
                    height,
                    config,
                )
            })
            .collect(),
        contours_suppressed_background: plan.contours_suppressed_background,
    }
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
    let bg = config.background_color.unwrap_or((255, 255, 255));
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
    contours: &[Contour],
    contour_indices: &[usize],
    width: u32,
    height: u32,
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
        let simplified = simplify_closed(contour, config.simplification_tolerance);
        if simplified.len() < 3 {
            result.contours_simplified_away += 1;
            continue;
        }

        // Convert to float and apply corner-aware Laplacian smoothing
        // proportional to the configured strength.
        let float_pts: Vec<(f64, f64)> = simplified
            .iter()
            .map(|p| (p.x as f64, p.y as f64))
            .collect();
        let smoothed = if config.smoothing_strength > 0.01 {
            // Derive iteration count from strength: low strength => 1 pass,
            // higher strength => more passes for stronger staircase reduction.
            let iterations = 1 + (config.smoothing_strength * 2.0).floor() as u32;
            smooth_closed_contour_adaptive_multi(
                &float_pts,
                config.smoothing_strength * 0.5,
                iterations,
                config.corner_sensitivity,
            )
        } else {
            float_pts
        };

        // Check minimum area using smoothed coordinates
        let area = polygon_area_f64(&smoothed);
        if area < config.min_region_area {
            result.contours_filtered_min_area += 1;
            continue;
        }

        let geometry = if config.smoothing_strength > 0.01 {
            // Use cubic Bezier curves for smoother output
            build_bezier_path(
                &smoothed,
                config.smoothing_strength,
                config.corner_sensitivity,
                width,
                height,
            )
        } else {
            // Use straight line segments
            build_linear_path(&smoothed, width, height)
        };

        parts.push(geometry.data);
        result.contours_emitted += 1;
        result.points_emitted += geometry.emitted_points;
        if contour_is_hole(&simplified) {
            result.holes_emitted += 1;
        }
    }

    result.data = parts.join(" ");
    result
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
                Point::new(4, 0),
                Point::new(4, 4),
                Point::new(0, 4),
            ]],
        }];

        let result = generate_svg_with_metrics(&regions, 4, 4, &config);
        assert!(result.svg.contains(" C "));
        assert_eq!(result.svg.matches(" C ").count(), 4);
        assert_eq!(result.metrics.regions_emitted, 1);
        assert_eq!(result.metrics.contours_emitted, 1);
        assert_eq!(result.metrics.points_emitted, 13);
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
                color: PaletteColor { r: 0, g: 0, b: 0 },
                contours: vec![vec![
                    Point::new(0, 0),
                    Point::new(8, 0),
                    Point::new(8, 8),
                    Point::new(0, 8),
                ]],
            },
        ];

        let svg = generate_svg(&regions, 8, 8, &config);
        let background_index = svg.find("fill=\"#000000\"").unwrap();
        let detail_index = svg.find("fill=\"#ff0000\"").unwrap();

        assert!(background_index < detail_index);
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
        assert!(geometry.data.contains("C 0.24 0.00, 3.76 0.00, 4.00 0.00"));
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

        let result = build_path_data_from_indices(&contours, &[0], 4, 4, &config);

        assert!(result.data.is_empty());
        assert_eq!(result.contours_emitted, 0);
        assert_eq!(result.contours_simplified_away, 1);
        assert_eq!(result.contours_filtered_min_area, 0);
    }
}
