//! SVG generation from traced contours and Bezier paths.
//!
//! Produces valid, clean SVG markup with proper viewBox, hole-preserving path
//! elements, and deterministic output.

use crate::config::TracingConfig;
use crate::pipeline::contour::{contour_is_hole, Contour, Point};
use crate::pipeline::curves::fit_cubic_beziers;
use crate::pipeline::segment::PaletteColor;
use crate::pipeline::simplify::simplify;

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
}

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

    // SVG header
    svg.push_str(&format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
"#
    ));

    // Background white rectangle
    svg.push_str(&format!(
        r#"  <rect width="{width}" height="{height}" fill="white"/>
"#
    ));

    // Each color region as a path group
    for region in regions {
        let hex = region.color.to_hex();

        let path_result = build_path_data(&region.contours, config);
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

    svg.push_str("</svg>\n");
    SvgBuildResult { svg, metrics }
}

/// Build SVG path data string from a list of contours.
fn build_path_data(contours: &[Contour], config: &TracingConfig) -> PathBuildResult {
    let mut parts = Vec::new();
    let mut result = PathBuildResult::default();

    for contour in contours {
        if contour.len() < 3 {
            continue;
        }

        // Simplify the polygon
        let simplified = simplify(contour, config.simplification_tolerance);
        if simplified.len() < 3 {
            continue;
        }

        // Check minimum area
        let area = polygon_area(&simplified);
        if area < config.min_region_area {
            continue;
        }

        let path = if config.smoothing_strength > 0.01 {
            // Use cubic Bezier curves for smoother output
            build_bezier_path(
                &simplified,
                config.smoothing_strength,
                config.corner_sensitivity,
            )
        } else {
            // Use straight line segments
            build_linear_path(&simplified)
        };

        parts.push(path);
        result.contours_emitted += 1;
        result.points_emitted += simplified.len();
        if contour_is_hole(&simplified) {
            result.holes_emitted += 1;
        }
    }

    result.data = parts.join(" ");
    result
}

/// Build a path using straight line segments.
fn build_linear_path(points: &[Point]) -> String {
    let mut d = String::new();
    for (i, p) in points.iter().enumerate() {
        if i == 0 {
            d.push_str(&format!("M {:.1} {:.1}", p.x as f64, p.y as f64));
        } else {
            d.push_str(&format!(" L {:.1} {:.1}", p.x as f64, p.y as f64));
        }
    }
    d.push_str(" Z");
    d
}

/// Build a path using cubic Bezier curves.
fn build_bezier_path(points: &[Point], smoothing: f64, corner_sensitivity: f64) -> String {
    let beziers = fit_cubic_beziers(points, smoothing, corner_sensitivity);
    if beziers.is_empty() {
        return build_linear_path(points);
    }

    let mut d = String::new();
    d.push_str(&format!("M {:.2} {:.2}", beziers[0].p0.0, beziers[0].p0.1));

    for bez in &beziers {
        d.push_str(&format!(
            " C {:.2} {:.2}, {:.2} {:.2}, {:.2} {:.2}",
            bez.p1.0, bez.p1.1, bez.p2.0, bez.p2.1, bez.p3.0, bez.p3.1
        ));
    }
    d.push_str(" Z");
    d
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::contour::Point;

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
}
