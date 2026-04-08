//! Contour vertex smoothing via Laplacian relaxation.
//!
//! Applies weighted averaging to shift grid-aligned contour vertices off the
//! integer grid, reducing staircase artifacts before Bezier curve fitting.
//! The adaptive variant dampens smoothing near sharp corners so the pipeline
//! preserves more structure while still softening pixel stair-steps. Larger
//! contours also receive a deterministic area-compensation pass after each
//! smoothing iteration so broad fills keep their apparent footprint better.

use crate::pipeline::curves::corner_cosine;

const AREA_PRESERVATION_MIN_AREA: f64 = 64.0;
const AREA_PRESERVATION_FULL_AREA: f64 = 256.0;
const AREA_PRESERVATION_SCALE_EPSILON: f64 = 1e-3;

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
#[cfg(test)]
pub(crate) fn smooth_closed_contour_multi(
    points: &[(f64, f64)],
    weight: f64,
    iterations: u32,
) -> Vec<(f64, f64)> {
    smooth_closed_contour_with_corner_sensitivity(points, weight, iterations, 0.0)
}

/// Apply multiple adaptive Laplacian smoothing passes to a closed polygon.
///
/// The base `weight` still controls the overall smoothing strength, but the
/// per-vertex weight is reduced near sharp corners as `corner_sensitivity`
/// increases. A sensitivity of `0.0` matches uniform smoothing, while `1.0`
/// strongly protects sharp turns from being rounded away.
pub(crate) fn smooth_closed_contour_adaptive_multi(
    points: &[(f64, f64)],
    weight: f64,
    iterations: u32,
    corner_sensitivity: f64,
) -> Vec<(f64, f64)> {
    smooth_closed_contour_with_corner_sensitivity(points, weight, iterations, corner_sensitivity)
}

fn smooth_closed_contour_with_corner_sensitivity(
    points: &[(f64, f64)],
    weight: f64,
    iterations: u32,
    corner_sensitivity: f64,
) -> Vec<(f64, f64)> {
    let n = points.len();
    if n < 3 || weight <= 0.0 || iterations == 0 {
        return points.to_vec();
    }

    let w = weight.clamp(0.0, 1.0);
    let sensitivity = corner_sensitivity.clamp(0.0, 1.0);
    let mut current = points.to_vec();

    for _ in 0..iterations {
        let smoothed = laplacian_smooth_closed_contour_step(&current, n, w, sensitivity);
        current = preserve_large_contour_area(&current, &smoothed);
    }

    current
}

fn laplacian_smooth_closed_contour_step(
    points: &[(f64, f64)],
    point_count: usize,
    weight: f64,
    corner_sensitivity: f64,
) -> Vec<(f64, f64)> {
    (0..point_count)
        .map(|i| {
            let prev = points[(i + point_count - 1) % point_count];
            let curr = points[i];
            let next = points[(i + 1) % point_count];
            let avg_x = (prev.0 + next.0) * 0.5;
            let avg_y = (prev.1 + next.1) * 0.5;
            let local_weight =
                weight * adaptive_corner_weight(prev, curr, next, corner_sensitivity);

            (
                curr.0 + local_weight * (avg_x - curr.0),
                curr.1 + local_weight * (avg_y - curr.1),
            )
        })
        .collect()
}

fn preserve_large_contour_area(
    reference: &[(f64, f64)],
    smoothed: &[(f64, f64)],
) -> Vec<(f64, f64)> {
    if reference.len() < 3 || reference.len() != smoothed.len() {
        return smoothed.to_vec();
    }

    let reference_area = polygon_area_f64(reference);
    if reference_area < AREA_PRESERVATION_MIN_AREA {
        return smoothed.to_vec();
    }

    let smoothed_area = polygon_area_f64(smoothed);
    if smoothed_area <= f64::EPSILON {
        return smoothed.to_vec();
    }

    let raw_scale = (reference_area / smoothed_area).sqrt();
    if !raw_scale.is_finite() {
        return smoothed.to_vec();
    }

    let blend = remap_clamped(
        reference_area,
        AREA_PRESERVATION_MIN_AREA,
        AREA_PRESERVATION_FULL_AREA,
        0.0,
        1.0,
    );
    if blend <= 0.0 {
        return smoothed.to_vec();
    }

    let scale = 1.0 + ((raw_scale - 1.0) * blend);
    if (scale - 1.0).abs() <= AREA_PRESERVATION_SCALE_EPSILON {
        return smoothed.to_vec();
    }

    let centroid = polygon_centroid(smoothed).unwrap_or_else(|| average_point(smoothed));
    smoothed
        .iter()
        .map(|&(x, y)| {
            (
                centroid.0 + ((x - centroid.0) * scale),
                centroid.1 + ((y - centroid.1) * scale),
            )
        })
        .collect()
}

fn adaptive_corner_weight(
    previous: (f64, f64),
    current: (f64, f64),
    next: (f64, f64),
    sensitivity: f64,
) -> f64 {
    if sensitivity <= 0.0 {
        return 1.0;
    }

    let Some(cos_angle) = corner_cosine(previous, current, next) else {
        return 1.0;
    };

    let straightness = ((cos_angle + 1.0) * 0.5).clamp(0.0, 1.0);
    let protected_weight = straightness.powf(1.0 + (sensitivity * 4.0));

    1.0 - (sensitivity * (1.0 - protected_weight))
}

fn polygon_area_f64(points: &[(f64, f64)]) -> f64 {
    let point_count = points.len();
    if point_count < 3 {
        return 0.0;
    }

    let mut area = 0.0;
    for index in 0..point_count {
        let next_index = (index + 1) % point_count;
        area += points[index].0 * points[next_index].1;
        area -= points[next_index].0 * points[index].1;
    }

    (area * 0.5).abs()
}

fn polygon_signed_area_f64(points: &[(f64, f64)]) -> f64 {
    let point_count = points.len();
    if point_count < 3 {
        return 0.0;
    }

    let mut area = 0.0;
    for index in 0..point_count {
        let next_index = (index + 1) % point_count;
        area += points[index].0 * points[next_index].1;
        area -= points[next_index].0 * points[index].1;
    }

    area * 0.5
}

fn polygon_centroid(points: &[(f64, f64)]) -> Option<(f64, f64)> {
    let signed_area = polygon_signed_area_f64(points);
    if signed_area.abs() <= f64::EPSILON {
        return None;
    }

    let mut centroid_x = 0.0;
    let mut centroid_y = 0.0;

    for index in 0..points.len() {
        let next_index = (index + 1) % points.len();
        let cross =
            (points[index].0 * points[next_index].1) - (points[next_index].0 * points[index].1);
        centroid_x += (points[index].0 + points[next_index].0) * cross;
        centroid_y += (points[index].1 + points[next_index].1) * cross;
    }

    let factor = 1.0 / (6.0 * signed_area);
    Some((centroid_x * factor, centroid_y * factor))
}

fn average_point(points: &[(f64, f64)]) -> (f64, f64) {
    let point_count = points.len().max(1) as f64;
    let sum_x = points.iter().map(|point| point.0).sum::<f64>();
    let sum_y = points.iter().map(|point| point.1).sum::<f64>();
    (sum_x / point_count, sum_y / point_count)
}

fn remap_clamped(value: f64, in_min: f64, in_max: f64, out_min: f64, out_max: f64) -> f64 {
    if in_max <= in_min {
        return out_max;
    }

    let t = ((value - in_min) / (in_max - in_min)).clamp(0.0, 1.0);
    out_min + ((out_max - out_min) * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn displacement(a: (f64, f64), b: (f64, f64)) -> f64 {
        let dx = a.0 - b.0;
        let dy = a.1 - b.1;
        (dx * dx + dy * dy).sqrt()
    }

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

    #[test]
    fn adaptive_zero_corner_sensitivity_matches_uniform_smoothing() {
        let pts = vec![
            (0.0, 0.0),
            (4.0, 0.0),
            (6.0, 1.0),
            (8.0, 0.0),
            (8.0, 6.0),
            (0.0, 6.0),
        ];

        let uniform = smooth_closed_contour_multi(&pts, 0.4, 2);
        let adaptive = smooth_closed_contour_adaptive_multi(&pts, 0.4, 2, 0.0);

        for i in 0..pts.len() {
            assert!(
                (uniform[i].0 - adaptive[i].0).abs() < 1e-10,
                "x mismatch at vertex {i}"
            );
            assert!(
                (uniform[i].1 - adaptive[i].1).abs() < 1e-10,
                "y mismatch at vertex {i}"
            );
        }
    }

    #[test]
    fn adaptive_smoothing_preserves_sharp_corners_more_than_gentle_bends() {
        let pts = vec![
            (0.0, 0.0),
            (3.0, 0.0),
            (6.0, 0.2),
            (9.0, 0.0),
            (9.0, 6.0),
            (0.0, 6.0),
        ];

        let uniform = smooth_closed_contour_multi(&pts, 0.45, 1);
        let adaptive = smooth_closed_contour_adaptive_multi(&pts, 0.45, 1, 1.0);

        let corner_index = 3;
        let gentle_index = 2;
        let uniform_corner_displacement = displacement(uniform[corner_index], pts[corner_index]);
        let adaptive_corner_displacement = displacement(adaptive[corner_index], pts[corner_index]);
        let adaptive_gentle_displacement = displacement(adaptive[gentle_index], pts[gentle_index]);

        assert!(
            adaptive_corner_displacement < uniform_corner_displacement,
            "adaptive smoothing should reduce corner drift"
        );
        assert!(
            adaptive_gentle_displacement > adaptive_corner_displacement,
            "gentle bends should still smooth more than sharp corners"
        );
    }

    #[test]
    fn area_preservation_reduces_large_contour_shrinkage() {
        let pts = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];

        let raw = laplacian_smooth_closed_contour_step(&pts, pts.len(), 0.5, 0.0);
        let preserved = smooth_closed_contour_multi(&pts, 0.5, 1);

        let original_area = polygon_area_f64(&pts);
        let raw_area_error = (polygon_area_f64(&raw) - original_area).abs();
        let preserved_area_error = (polygon_area_f64(&preserved) - original_area).abs();

        assert!(
            preserved_area_error < raw_area_error,
            "area-preserving smoothing should reduce large-contour shrinkage"
        );
    }

    #[test]
    fn area_preservation_skips_small_contours() {
        let pts = vec![(0.0, 0.0), (4.0, 0.0), (4.0, 4.0), (0.0, 4.0)];

        let raw = laplacian_smooth_closed_contour_step(&pts, pts.len(), 0.5, 0.0);
        let preserved = smooth_closed_contour_multi(&pts, 0.5, 1);

        for index in 0..pts.len() {
            assert!((raw[index].0 - preserved[index].0).abs() < 1e-10);
            assert!((raw[index].1 - preserved[index].1).abs() < 1e-10);
        }
    }
}
