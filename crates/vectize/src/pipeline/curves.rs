//! Bezier curve fitting and smoothing for polygon paths.
//!
//! Converts simplified polylines into smooth cubic Bezier splines
//! for cleaner SVG output.

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
pub fn fit_cubic_beziers(points: &[Point], smoothing: f64) -> Vec<CubicBezier> {
    if points.len() < 2 {
        return Vec::new();
    }

    let tension = smoothing.clamp(0.0, 1.0) * 0.4;
    let pts: Vec<(f64, f64)> = points.iter().map(|p| (p.x as f64, p.y as f64)).collect();
    let n = pts.len();

    if n == 2 {
        // Single straight segment
        return vec![CubicBezier {
            p0: pts[0],
            p1: lerp2(pts[0], pts[1], 1.0 / 3.0),
            p2: lerp2(pts[0], pts[1], 2.0 / 3.0),
            p3: pts[1],
        }];
    }

    let mut segments = Vec::new();

    for i in 0..n - 1 {
        let p0 = pts[i];
        let p3 = pts[i + 1];

        // Catmull-Rom tangent vectors
        let prev = if i > 0 { pts[i - 1] } else { p0 };
        let next = if i + 2 < n { pts[i + 2] } else { p3 };

        let p1 = (
            p0.0 + tension * (p3.0 - prev.0),
            p0.1 + tension * (p3.1 - prev.1),
        );
        let p2 = (
            p3.0 - tension * (next.0 - p0.0),
            p3.1 - tension * (next.1 - p0.1),
        );

        segments.push(CubicBezier { p0, p1, p2, p3 });
    }

    segments
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
        let beziers = fit_cubic_beziers(&pts, 0.5);
        assert_eq!(beziers.len(), 1);
    }

    #[test]
    fn n_points_produce_n_minus_1_segments() {
        let pts: Vec<Point> = (0..5).map(|i| Point::new(i * 10, 0)).collect();
        let beziers = fit_cubic_beziers(&pts, 0.5);
        assert_eq!(beziers.len(), 4);
    }
}
