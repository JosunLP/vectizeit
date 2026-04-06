//! Contour vertex smoothing via Laplacian relaxation.
//!
//! Applies a single pass of weighted averaging to shift grid-aligned contour
//! vertices off the integer grid, reducing staircase artifacts before Bezier
//! curve fitting.

/// Apply one pass of Laplacian smoothing to a closed polygon.
///
/// Each vertex moves toward the midpoint of its two neighbors by the given
/// `weight` (clamped to `[0, 1]`).  A weight of `0.0` leaves the contour
/// unchanged; `0.5` averages the vertex equally with its neighbors' midpoint.
///
/// Returns the smoothed polygon with the same number of vertices.
#[cfg(test)]
pub(crate) fn smooth_closed_contour(points: &[(f64, f64)], weight: f64) -> Vec<(f64, f64)> {
    smooth_closed_contour_multi(points, weight, 1)
}

/// Apply multiple passes of Laplacian smoothing to a closed polygon.
///
/// `iterations` controls how many passes are applied. More iterations produce
/// stronger smoothing, useful for high-detail images with pronounced staircase
/// artifacts. An iteration count of `0` returns the input unchanged.
pub(crate) fn smooth_closed_contour_multi(
    points: &[(f64, f64)],
    weight: f64,
    iterations: u32,
) -> Vec<(f64, f64)> {
    let n = points.len();
    if n < 3 || weight <= 0.0 || iterations == 0 {
        return points.to_vec();
    }

    let w = weight.clamp(0.0, 1.0);
    let mut current = points.to_vec();

    for _ in 0..iterations {
        current = (0..n)
            .map(|i| {
                let prev = current[(i + n - 1) % n];
                let curr = current[i];
                let next = current[(i + 1) % n];
                let avg_x = (prev.0 + next.0) * 0.5;
                let avg_y = (prev.1 + next.1) * 0.5;
                (curr.0 + w * (avg_x - curr.0), curr.1 + w * (avg_y - curr.1))
            })
            .collect();
    }

    current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_weight_returns_unchanged() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let smoothed = smooth_closed_contour(&pts, 0.0);
        assert_eq!(smoothed, pts);
    }

    #[test]
    fn negative_weight_returns_unchanged() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)];
        let smoothed = smooth_closed_contour(&pts, -0.5);
        assert_eq!(smoothed, pts);
    }

    #[test]
    fn too_few_points_returns_unchanged() {
        let pts = vec![(0.0, 0.0), (5.0, 5.0)];
        let smoothed = smooth_closed_contour(&pts, 0.5);
        assert_eq!(smoothed, pts);
    }

    #[test]
    fn empty_contour_returns_empty() {
        let smoothed = smooth_closed_contour(&[], 0.5);
        assert!(smoothed.is_empty());
    }

    #[test]
    fn smoothing_preserves_vertex_count() {
        let pts: Vec<(f64, f64)> = (0..20)
            .map(|i| {
                let angle = (i as f64) * std::f64::consts::TAU / 20.0;
                (10.0 + 5.0 * angle.cos(), 10.0 + 5.0 * angle.sin())
            })
            .collect();
        let smoothed = smooth_closed_contour(&pts, 0.5);
        assert_eq!(smoothed.len(), pts.len());
    }

    #[test]
    fn staircase_vertices_move_off_grid() {
        // A grid-aligned staircase pattern
        let pts = vec![
            (0.0, 0.0),
            (1.0, 0.0),
            (1.0, 1.0),
            (2.0, 1.0),
            (2.0, 2.0),
            (0.0, 2.0),
        ];
        let smoothed = smooth_closed_contour(&pts, 0.5);

        // Interior staircase points should have moved off the integer grid
        let moved_count = smoothed
            .iter()
            .enumerate()
            .filter(|(i, p)| {
                let orig = pts[*i];
                (p.0 - orig.0).abs() > 1e-10 || (p.1 - orig.1).abs() > 1e-10
            })
            .count();
        assert!(
            moved_count > 0,
            "smoothing should move at least some vertices"
        );
    }

    #[test]
    fn regular_polygon_centroid_stays_stable() {
        // A regular hexagon should preserve its centroid after smoothing
        let pts: Vec<(f64, f64)> = (0..6)
            .map(|i| {
                let angle = (i as f64) * std::f64::consts::TAU / 6.0;
                (10.0 + 5.0 * angle.cos(), 10.0 + 5.0 * angle.sin())
            })
            .collect();
        let smoothed = smooth_closed_contour(&pts, 0.25);

        let orig_cx: f64 = pts.iter().map(|p| p.0).sum::<f64>() / pts.len() as f64;
        let orig_cy: f64 = pts.iter().map(|p| p.1).sum::<f64>() / pts.len() as f64;
        let smooth_cx: f64 = smoothed.iter().map(|p| p.0).sum::<f64>() / smoothed.len() as f64;
        let smooth_cy: f64 = smoothed.iter().map(|p| p.1).sum::<f64>() / smoothed.len() as f64;

        assert!(
            (smooth_cx - orig_cx).abs() < 0.01,
            "centroid x should be stable"
        );
        assert!(
            (smooth_cy - orig_cy).abs() < 0.01,
            "centroid y should be stable"
        );
    }

    #[test]
    fn weight_above_one_is_clamped() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (5.0, 8.66)];
        let result_1_0 = smooth_closed_contour(&pts, 1.0);
        let result_1_5 = smooth_closed_contour(&pts, 1.5);

        // weight 1.5 should produce the same result as 1.0 (clamped)
        for i in 0..pts.len() {
            assert!(
                (result_1_5[i].0 - result_1_0[i].0).abs() < 1e-10,
                "x mismatch at vertex {i}"
            );
            assert!(
                (result_1_5[i].1 - result_1_0[i].1).abs() < 1e-10,
                "y mismatch at vertex {i}"
            );
        }
    }

    #[test]
    fn higher_weight_produces_stronger_smoothing() {
        let pts = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];
        let low = smooth_closed_contour(&pts, 0.1);
        let high = smooth_closed_contour(&pts, 0.5);

        // Higher weight should move points further from their original positions
        let low_displacement: f64 = low
            .iter()
            .zip(pts.iter())
            .map(|(s, o)| (s.0 - o.0).powi(2) + (s.1 - o.1).powi(2))
            .sum();
        let high_displacement: f64 = high
            .iter()
            .zip(pts.iter())
            .map(|(s, o)| (s.0 - o.0).powi(2) + (s.1 - o.1).powi(2))
            .sum();
        assert!(
            high_displacement > low_displacement,
            "higher weight should produce more displacement"
        );
    }

    #[test]
    fn multi_pass_zero_iterations_returns_unchanged() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let result = smooth_closed_contour_multi(&pts, 0.5, 0);
        assert_eq!(result, pts);
    }

    #[test]
    fn multi_pass_one_iteration_matches_single_pass() {
        let pts = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let single = smooth_closed_contour(&pts, 0.5);
        let multi = smooth_closed_contour_multi(&pts, 0.5, 1);
        for i in 0..pts.len() {
            assert!(
                (single[i].0 - multi[i].0).abs() < 1e-10,
                "x mismatch at vertex {i}"
            );
            assert!(
                (single[i].1 - multi[i].1).abs() < 1e-10,
                "y mismatch at vertex {i}"
            );
        }
    }

    #[test]
    fn multi_pass_more_iterations_produce_stronger_smoothing() {
        let pts = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];
        let one_pass = smooth_closed_contour_multi(&pts, 0.3, 1);
        let three_pass = smooth_closed_contour_multi(&pts, 0.3, 3);

        let one_disp: f64 = one_pass
            .iter()
            .zip(pts.iter())
            .map(|(s, o)| (s.0 - o.0).powi(2) + (s.1 - o.1).powi(2))
            .sum();
        let three_disp: f64 = three_pass
            .iter()
            .zip(pts.iter())
            .map(|(s, o)| (s.0 - o.0).powi(2) + (s.1 - o.1).powi(2))
            .sum();
        assert!(
            three_disp > one_disp,
            "more iterations should produce more displacement"
        );
    }
}
