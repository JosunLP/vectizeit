//! Path simplification using the Ramer-Douglas-Peucker algorithm.

use crate::pipeline::contour::Point;

/// Simplify a polyline using the Ramer-Douglas-Peucker algorithm.
///
/// `tolerance` controls how aggressively the path is simplified.
/// A higher tolerance removes more points.
pub fn simplify(points: &[Point], tolerance: f64) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut result = Vec::new();
    rdp(points, tolerance, &mut result);
    result
}

fn rdp(points: &[Point], tolerance: f64, result: &mut Vec<Point>) {
    let n = points.len();
    if n < 2 {
        if let Some(p) = points.first() {
            result.push(*p);
        }
        return;
    }

    let start = points[0];
    let end = points[n - 1];

    // Find the point with the maximum perpendicular distance
    let (max_dist, max_idx) = points[1..n - 1]
        .iter()
        .enumerate()
        .map(|(i, &p)| (perpendicular_distance(p, start, end), i + 1))
        .fold(
            (0.0f64, 0),
            |(md, mi), (d, i)| {
                if d > md {
                    (d, i)
                } else {
                    (md, mi)
                }
            },
        );

    if max_dist > tolerance {
        rdp(&points[..=max_idx], tolerance, result);
        result.pop(); // avoid duplicate at junction
        rdp(&points[max_idx..], tolerance, result);
    } else {
        result.push(start);
        result.push(end);
    }
}

/// Perpendicular distance from point `p` to the line defined by `a` and `b`.
fn perpendicular_distance(p: Point, a: Point, b: Point) -> f64 {
    let dx = (b.x - a.x) as f64;
    let dy = (b.y - a.y) as f64;
    let len = (dx * dx + dy * dy).sqrt();

    if len < 1e-10 {
        let dpx = (p.x - a.x) as f64;
        let dpy = (p.y - a.y) as f64;
        return (dpx * dpx + dpy * dpy).sqrt();
    }

    let num = ((b.y - a.y) as f64 * p.x as f64 - (b.x - a.x) as f64 * p.y as f64
        + b.x as f64 * a.y as f64
        - b.y as f64 * a.x as f64)
        .abs();
    num / len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simplify_line() {
        // Points on a straight line should reduce to just start and end
        let points: Vec<Point> = (0..10).map(|i| Point::new(i, i)).collect();
        let simplified = simplify(&points, 0.5);
        assert!(simplified.len() <= 3);
    }

    #[test]
    fn simplify_preserves_important_corners() {
        let points = vec![
            Point::new(0, 0),
            Point::new(5, 0),
            Point::new(5, 5), // corner
            Point::new(10, 5),
        ];
        let simplified = simplify(&points, 0.5);
        // The corner at (5,0) or (5,5) must be preserved
        assert!(simplified.len() >= 3);
    }
}
