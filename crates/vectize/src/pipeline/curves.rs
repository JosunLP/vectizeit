//! Bezier curve fitting and smoothing for polygon paths.
//!
//! Converts simplified polylines into smooth cubic Bezier splines
//! for cleaner SVG output. Supports corner detection to preserve
//! sharp angles while smoothing gentle curves and keeping corner
//! handles aligned with traced edges.

use crate::pipeline::contour::Point;

/// A cubic Bezier segment defined by four control points.
#[derive(Debug, Clone)]
pub struct CubicBezier {
    pub p0: (f64, f64),
    pub p1: (f64, f64),
    pub p2: (f64, f64),
    pub p3: (f64, f64),
}

/// Fit a smooth cubic Bezier spline through the given points.
///
/// `smoothing` controls the tension: 0.0 = straight lines, 1.0 = maximum smoothing.
/// `corner_sensitivity` controls corner detection: 0.0 = smooth all, 1.0 = preserve all corners.
pub fn fit_cubic_beziers(
    points: &[Point],
    smoothing: f64,
    corner_sensitivity: f64,
) -> Vec<CubicBezier> {
    fit_cubic_beziers_impl(points, smoothing, corner_sensitivity, false)
}

pub(crate) fn fit_closed_cubic_beziers_f64(
    points: &[(f64, f64)],
    smoothing: f64,
    corner_sensitivity: f64,
) -> Vec<CubicBezier> {
    if points.len() < 3 {
        return Vec::new();
    }
    fit_beziers_core(points, smoothing, corner_sensitivity, true)
}

pub(crate) fn corner_cosine(
    previous: (f64, f64),
    current: (f64, f64),
    next: (f64, f64),
) -> Option<f64> {
    let (ax, ay) = (current.0 - previous.0, current.1 - previous.1);
    let (bx, by) = (next.0 - current.0, next.1 - current.1);

    let len_a = (ax * ax + ay * ay).sqrt();
    let len_b = (bx * bx + by * by).sqrt();

    if len_a < 1e-10 || len_b < 1e-10 {
        return None;
    }

    Some(((ax * bx + ay * by) / (len_a * len_b)).clamp(-1.0, 1.0))
}

fn fit_cubic_beziers_impl(
    points: &[Point],
    smoothing: f64,
    corner_sensitivity: f64,
    closed: bool,
) -> Vec<CubicBezier> {
    let pts: Vec<(f64, f64)> = points.iter().map(|p| (p.x as f64, p.y as f64)).collect();
    fit_beziers_core(&pts, smoothing, corner_sensitivity, closed)
}

fn fit_beziers_core(
    pts: &[(f64, f64)],
    smoothing: f64,
    corner_sensitivity: f64,
    closed: bool,
) -> Vec<CubicBezier> {
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }

    let tension = smoothing.clamp(0.0, 1.0) * 0.4;

    if !closed && n == 2 {
        // Single straight segment
        return vec![CubicBezier {
            p0: pts[0],
            p1: lerp2(pts[0], pts[1], 1.0 / 3.0),
            p2: lerp2(pts[0], pts[1], 2.0 / 3.0),
            p3: pts[1],
        }];
    }

    // Detect corners based on angle between consecutive segments
    let corners = if closed {
        detect_closed_corners(pts, corner_sensitivity)
    } else {
        detect_corners(pts, corner_sensitivity)
    };

    let mut segments = Vec::new();
    let segment_count = if closed { n } else { n - 1 };

    for i in 0..segment_count {
        let p0 = pts[i];
        let next_idx = if closed { (i + 1) % n } else { i + 1 };
        let p3 = pts[next_idx];

        let start_tension = endpoint_tension(tension, corners[i]);
        let end_tension = endpoint_tension(tension, corners[next_idx]);

        // Catmull-Rom tangent vectors with edge-aligned handles at detected corners.
        let prev = if closed {
            pts[(i + n - 1) % n]
        } else if i > 0 {
            pts[i - 1]
        } else {
            p0
        };
        let next = if closed {
            pts[(i + 2) % n]
        } else if i + 2 < n {
            pts[i + 2]
        } else {
            p3
        };

        let p1 = start_control_point(prev, p0, p3, start_tension, corners[i]);
        let p2 = end_control_point(p0, p3, next, end_tension, corners[next_idx]);

        segments.push(CubicBezier { p0, p1, p2, p3 });
    }

    segments
}

fn endpoint_tension(base_tension: f64, preserve_corner: bool) -> f64 {
    if preserve_corner {
        base_tension * 0.25
    } else {
        base_tension
    }
}

fn start_control_point(
    previous: (f64, f64),
    start: (f64, f64),
    end: (f64, f64),
    tension: f64,
    preserve_corner: bool,
) -> (f64, f64) {
    if preserve_corner {
        lerp2(start, end, tension)
    } else {
        (
            start.0 + tension * (end.0 - previous.0),
            start.1 + tension * (end.1 - previous.1),
        )
    }
}

fn end_control_point(
    start: (f64, f64),
    end: (f64, f64),
    next: (f64, f64),
    tension: f64,
    preserve_corner: bool,
) -> (f64, f64) {
    if preserve_corner {
        lerp2(end, start, tension)
    } else {
        (
            end.0 - tension * (next.0 - start.0),
            end.1 - tension * (next.1 - start.1),
        )
    }
}

fn detect_closed_corners(pts: &[(f64, f64)], sensitivity: f64) -> Vec<bool> {
    let n = pts.len();
    let mut corners = vec![false; n];

    if n < 3 || sensitivity <= 0.0 {
        return corners;
    }

    let cos_threshold = corner_cos_threshold(sensitivity);

    for i in 0..n {
        let prev = pts[(i + n - 1) % n];
        let next = pts[(i + 1) % n];
        corners[i] = is_corner(prev, pts[i], next, cos_threshold);
    }

    corners
}

/// Detect corner points based on the angle between consecutive segments.
///
/// Returns a boolean for each point: `true` if the point is a sharp corner.
/// `sensitivity`: 0.0 = no corners detected, 1.0 = aggressively detect corners.
fn detect_corners(pts: &[(f64, f64)], sensitivity: f64) -> Vec<bool> {
    let n = pts.len();
    let mut corners = vec![false; n];

    if n < 3 || sensitivity <= 0.0 {
        return corners;
    }

    let cos_threshold = corner_cos_threshold(sensitivity);

    for i in 1..n - 1 {
        corners[i] = is_corner(pts[i - 1], pts[i], pts[i + 1], cos_threshold);
    }

    corners
}

fn corner_cos_threshold(sensitivity: f64) -> f64 {
    // Angle threshold: higher sensitivity → more corners detected (higher threshold)
    // sensitivity=1.0 → threshold=cos(60°) = 0.5 (very aggressive)
    // sensitivity=0.6 → threshold=cos(84°) ≈ 0.1 (moderate, catches right angles)
    // sensitivity=0.0 → threshold=cos(120°) = -0.5 (only sharp U-turns)
    sensitivity - 0.5
}

fn is_corner(
    previous: (f64, f64),
    current: (f64, f64),
    next: (f64, f64),
    cos_threshold: f64,
) -> bool {
    corner_cosine(previous, current, next).is_some_and(|cos_angle| cos_angle < cos_threshold)
}

fn lerp2(a: (f64, f64), b: (f64, f64), t: f64) -> (f64, f64) {
    (a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_points_produce_one_segment() {
        let pts = vec![Point::new(0, 0), Point::new(10, 10)];
        let beziers = fit_cubic_beziers(&pts, 0.5, 0.6);
        assert_eq!(beziers.len(), 1);
    }

    #[test]
    fn n_points_produce_n_minus_1_segments() {
        let pts: Vec<Point> = (0..5).map(|i| Point::new(i * 10, 0)).collect();
        let beziers = fit_cubic_beziers(&pts, 0.5, 0.6);
        assert_eq!(beziers.len(), 4);
    }

    #[test]
    fn corner_detection_finds_right_angle() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let corners = detect_corners(&pts, 0.6);
        assert!(corners[1]); // The 90-degree corner at (10, 0)
    }

    #[test]
    fn corner_detection_ignores_gentle_curves() {
        // Nearly straight line
        let pts = vec![(0.0, 0.0), (5.0, 0.1), (10.0, 0.0)];
        let corners = detect_corners(&pts, 0.6);
        assert!(!corners[1]); // Not a corner
    }

    #[test]
    fn zero_sensitivity_detects_no_corners() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let corners = detect_corners(&pts, 0.0);
        assert!(corners.iter().all(|&c| !c));
    }

    #[test]
    fn closed_square_produces_one_segment_per_edge() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];

        let beziers = fit_closed_cubic_beziers_f64(&pts, 0.5, 0.6);

        assert_eq!(beziers.len(), 4);
        assert_eq!(beziers.first().unwrap().p0, (0.0, 0.0));
        assert_eq!(beziers.last().unwrap().p3, (0.0, 0.0));
    }

    #[test]
    fn closed_corner_detection_wraps_around_contours() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let corners = detect_closed_corners(&pts, 0.6);

        assert!(corners.iter().all(|&corner| corner));
    }

    #[test]
    fn closed_square_corner_handles_stay_within_edges() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];

        let beziers = fit_closed_cubic_beziers_f64(&pts, 0.6, 0.6);

        for bezier in &beziers {
            for (x, y) in [bezier.p0, bezier.p1, bezier.p2, bezier.p3] {
                assert!(
                    (0.0..=10.0).contains(&x),
                    "x coordinate must stay in bounds: {x}"
                );
                assert!(
                    (0.0..=10.0).contains(&y),
                    "y coordinate must stay in bounds: {y}"
                );
            }
        }

        let top = &beziers[0];
        assert!(top.p1.1.abs() < 1e-12);
        assert!(top.p2.1.abs() < 1e-12);

        let right = &beziers[1];
        assert!((right.p1.0 - 10.0).abs() < 1e-12);
        assert!((right.p2.0 - 10.0).abs() < 1e-12);
    }

    #[test]
    fn corner_cosine_detects_straight_segments() {
        let cos_angle = corner_cosine((0.0, 0.0), (5.0, 0.0), (10.0, 0.0)).unwrap();

        assert!((cos_angle - 1.0).abs() < 1e-10);
    }
}
