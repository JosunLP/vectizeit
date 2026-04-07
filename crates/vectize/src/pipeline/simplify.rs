//! Path simplification using the Ramer-Douglas-Peucker algorithm.
//!
//! Provides both open-polyline and closed-polygon variants.
//! Closed-polygon simplification splits the polygon at its two most distant
//! vertices and simplifies each arc independently, avoiding artifacts at
//! arbitrary start points.

use crate::pipeline::contour::Point;

/// Simplify an open polyline using the Ramer-Douglas-Peucker algorithm.
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

/// Simplify a closed polygon using the Ramer-Douglas-Peucker algorithm.
///
/// Unlike [`simplify`], this correctly handles closed contours by splitting
/// the polygon at its two most distant vertices and simplifying each arc
/// independently.  This avoids preserving an arbitrary start vertex and
/// ensures the closing edge is evaluated for simplification.
pub fn simplify_closed(points: &[Point], tolerance: f64) -> Vec<Point> {
    let n = points.len();
    if n < 4 {
        return points.to_vec();
    }

    // Find approximate polygon diameter via two passes.
    let (a_idx, _) = farthest_from(points, 0);
    let (b_idx, _) = farthest_from(points, a_idx);

    // Ensure first < second for consistent slicing.
    let (first, second) = if a_idx <= b_idx {
        (a_idx, b_idx)
    } else {
        (b_idx, a_idx)
    };

    // Degenerate case: both endpoints are the same vertex.
    if first == second {
        return simplify(points, tolerance);
    }

    // Split the polygon into two open arcs that share the split vertices.
    let chain1 = &points[first..=second];
    let chain2: Vec<Point> = points[second..]
        .iter()
        .chain(points[..=first].iter())
        .copied()
        .collect();

    let simplified1 = simplify(chain1, tolerance);
    let simplified2 = simplify(&chain2, tolerance);

    // Combine the two arcs, removing duplicate split vertices.
    let mut result = simplified1;
    if simplified2.len() > 2 {
        result.extend_from_slice(&simplified2[1..simplified2.len() - 1]);
    }

    result
}

/// Simplify a closed polygon with floating-point coordinates using the
/// Ramer-Douglas-Peucker algorithm.
pub(crate) fn simplify_closed_f64(points: &[(f64, f64)], tolerance: f64) -> Vec<(f64, f64)> {
    let n = points.len();
    if n < 4 {
        return points.to_vec();
    }

    let (a_idx, _) = farthest_from_f64(points, 0);
    let (b_idx, _) = farthest_from_f64(points, a_idx);

    let (first, second) = if a_idx <= b_idx {
        (a_idx, b_idx)
    } else {
        (b_idx, a_idx)
    };

    if first == second {
        return simplify_f64(points, tolerance);
    }

    let chain1 = &points[first..=second];
    let chain2: Vec<(f64, f64)> = points[second..]
        .iter()
        .chain(points[..=first].iter())
        .copied()
        .collect();

    let simplified1 = simplify_f64(chain1, tolerance);
    let simplified2 = simplify_f64(&chain2, tolerance);

    let mut result = simplified1;
    if simplified2.len() > 2 {
        result.extend_from_slice(&simplified2[1..simplified2.len() - 1]);
    }

    result
}

/// Return the index of the point farthest from `points[from]`, together with
/// the squared distance.
fn farthest_from(points: &[Point], from: usize) -> (usize, i64) {
    let p = points[from];
    points
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let dx = (q.x - p.x) as i64;
            let dy = (q.y - p.y) as i64;
            (i, dx * dx + dy * dy)
        })
        .max_by_key(|(_, d)| *d)
        .unwrap_or((from, 0))
}

fn farthest_from_f64(points: &[(f64, f64)], from: usize) -> (usize, f64) {
    let p = points[from];
    points
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let dx = q.0 - p.0;
            let dy = q.1 - p.1;
            (i, dx * dx + dy * dy)
        })
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .unwrap_or((from, 0.0))
}

fn simplify_f64(points: &[(f64, f64)], tolerance: f64) -> Vec<(f64, f64)> {
    if points.len() < 3 {
        return points.to_vec();
    }

    let mut result = Vec::new();
    rdp_f64(points, tolerance, &mut result);
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

fn rdp_f64(points: &[(f64, f64)], tolerance: f64, result: &mut Vec<(f64, f64)>) {
    let n = points.len();
    if n < 2 {
        if let Some(point) = points.first() {
            result.push(*point);
        }
        return;
    }

    let start = points[0];
    let end = points[n - 1];

    let (max_dist, max_idx) = points[1..n - 1]
        .iter()
        .enumerate()
        .map(|(i, &point)| (perpendicular_distance_f64(point, start, end), i + 1))
        .fold((0.0f64, 0usize), |(max_dist, max_idx), (dist, idx)| {
            if dist > max_dist {
                (dist, idx)
            } else {
                (max_dist, max_idx)
            }
        });

    if max_dist > tolerance {
        rdp_f64(&points[..=max_idx], tolerance, result);
        result.pop();
        rdp_f64(&points[max_idx..], tolerance, result);
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

fn perpendicular_distance_f64(point: (f64, f64), start: (f64, f64), end: (f64, f64)) -> f64 {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let len = (dx * dx + dy * dy).sqrt();

    if len < 1e-10 {
        let dpx = point.0 - start.0;
        let dpy = point.1 - start.1;
        return (dpx * dpx + dpy * dpy).sqrt();
    }

    let num = (dy * point.0 - dx * point.1 + end.0 * start.1 - end.1 * start.0).abs();
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

    #[test]
    fn simplify_closed_removes_collinear_start_vertex() {
        // Closed square with an extra collinear point at the seam.
        // The open `simplify` would keep point[0] unconditionally;
        // `simplify_closed` should remove it because it lies on the edge.
        let points = vec![
            Point::new(5, 0), // collinear between (0,0) and (10,0)
            Point::new(10, 0),
            Point::new(10, 10),
            Point::new(0, 10),
            Point::new(0, 0),
        ];
        let simplified = simplify_closed(&points, 0.5);
        // The collinear midpoint (5,0) should be removed, leaving 4 vertices.
        assert_eq!(simplified.len(), 4);
    }

    #[test]
    fn simplify_closed_preserves_all_corners_of_a_square() {
        let points = vec![
            Point::new(0, 0),
            Point::new(10, 0),
            Point::new(10, 10),
            Point::new(0, 10),
        ];
        let simplified = simplify_closed(&points, 0.5);
        assert_eq!(simplified.len(), 4);
        assert!(simplified.contains(&Point::new(0, 0)));
        assert!(simplified.contains(&Point::new(10, 0)));
        assert!(simplified.contains(&Point::new(10, 10)));
        assert!(simplified.contains(&Point::new(0, 10)));
    }

    #[test]
    fn simplify_closed_handles_triangle() {
        // 3 points → returned as-is (below threshold for splitting).
        let points = vec![Point::new(0, 0), Point::new(5, 10), Point::new(10, 0)];
        let simplified = simplify_closed(&points, 1.0);
        assert_eq!(simplified.len(), 3);
    }

    #[test]
    fn simplify_closed_is_deterministic() {
        let points = vec![
            Point::new(0, 0),
            Point::new(3, 0),
            Point::new(5, 1),
            Point::new(10, 0),
            Point::new(10, 10),
            Point::new(0, 10),
        ];
        let a = simplify_closed(&points, 0.5);
        let b = simplify_closed(&points, 0.5);
        assert_eq!(a, b);
    }

    #[test]
    fn farthest_from_finds_opposite_corner() {
        let points = vec![
            Point::new(0, 0),
            Point::new(10, 0),
            Point::new(10, 10),
            Point::new(0, 10),
        ];
        let (idx, _) = farthest_from(&points, 0);
        assert_eq!(idx, 2); // (10,10) is farthest from (0,0)
    }

    #[test]
    fn simplify_closed_f64_preserves_square_corners() {
        let points = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];

        let simplified = simplify_closed_f64(&points, 0.5);

        assert_eq!(simplified.len(), 4);
        assert!(simplified.contains(&(0.0, 0.0)));
        assert!(simplified.contains(&(10.0, 0.0)));
        assert!(simplified.contains(&(10.0, 10.0)));
        assert!(simplified.contains(&(0.0, 10.0)));
    }

    #[test]
    fn simplify_closed_f64_collapses_nearly_collinear_float_wiggles() {
        let points = vec![
            (0.0, 0.0),
            (1.0, 0.04),
            (2.0, -0.03),
            (3.0, 0.02),
            (4.0, 0.0),
            (4.0, 4.0),
            (0.0, 4.0),
        ];

        let simplified = simplify_closed_f64(&points, 0.15);

        assert!(simplified.len() < points.len());
        assert!(simplified.len() >= 4);
    }
}
