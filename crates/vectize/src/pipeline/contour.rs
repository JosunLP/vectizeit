//! Contour extraction from labeled regions using deterministic grid-edge tracing.
//!
//! For each color region, the tracer walks the exposed pixel edges and stitches
//! them into closed polygon loops. This preserves interior holes and emits
//! deterministic contour winding for SVG generation.

use std::collections::{HashMap, HashSet};

use super::segment::SegmentationResult;

/// A 2D integer point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

/// A contour: a closed sequence of points.
pub type Contour = Vec<Point>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Edge {
    start: Point,
    end: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Right,
    Down,
    Left,
}

impl Direction {
    fn index(self) -> i32 {
        match self {
            Direction::Up => 0,
            Direction::Right => 1,
            Direction::Down => 2,
            Direction::Left => 3,
        }
    }
}

/// Extract contours for all color regions.
///
/// Returns a list of `(palette_index, contours)` pairs.
pub fn extract_contours(seg: &SegmentationResult) -> Vec<(u8, Vec<Contour>)> {
    let width = seg.width as usize;
    let height = seg.height as usize;

    let mut result = Vec::new();
    for color_idx in 0..seg.palette.len() as u8 {
        let contours = trace_color_contours(&seg.labels, width, height, color_idx);
        if !contours.is_empty() {
            result.push((color_idx, contours));
        }
    }

    result
}

/// Return `true` when the contour represents an interior hole.
pub fn contour_is_hole(contour: &Contour) -> bool {
    signed_area(contour) < 0.0
}

/// Calculate the signed area of a contour using the shoelace formula.
pub fn signed_area(contour: &Contour) -> f64 {
    if contour.len() < 3 {
        return 0.0;
    }

    let mut area = 0.0;
    for i in 0..contour.len() {
        let j = (i + 1) % contour.len();
        area += contour[i].x as f64 * contour[j].y as f64;
        area -= contour[j].x as f64 * contour[i].y as f64;
    }

    area / 2.0
}

/// Trace all contours for a single color label by extracting exposed pixel edges
/// and stitching them into loops.
fn trace_color_contours(labels: &[u8], width: usize, height: usize, target: u8) -> Vec<Contour> {
    let mask: Vec<bool> = labels.iter().map(|&label| label == target).collect();
    let edges = collect_boundary_edges(&mask, width, height);
    trace_loops(edges)
}

fn collect_boundary_edges(mask: &[bool], width: usize, height: usize) -> Vec<Edge> {
    let mut edges = Vec::new();

    for y in 0..height {
        for x in 0..width {
            if !mask[y * width + x] {
                continue;
            }

            let x = x as i32;
            let y = y as i32;

            if y == 0 || !mask[(y as usize - 1) * width + x as usize] {
                edges.push(Edge {
                    start: Point::new(x, y),
                    end: Point::new(x + 1, y),
                });
            }
            if x as usize + 1 >= width || !mask[y as usize * width + x as usize + 1] {
                edges.push(Edge {
                    start: Point::new(x + 1, y),
                    end: Point::new(x + 1, y + 1),
                });
            }
            if y as usize + 1 >= height || !mask[(y as usize + 1) * width + x as usize] {
                edges.push(Edge {
                    start: Point::new(x + 1, y + 1),
                    end: Point::new(x, y + 1),
                });
            }
            if x == 0 || !mask[y as usize * width + x as usize - 1] {
                edges.push(Edge {
                    start: Point::new(x, y + 1),
                    end: Point::new(x, y),
                });
            }
        }
    }

    edges.sort_unstable();
    edges
}

fn trace_loops(edges: Vec<Edge>) -> Vec<Contour> {
    let mut outgoing: HashMap<Point, Vec<Point>> = HashMap::new();
    for edge in &edges {
        outgoing.entry(edge.start).or_default().push(edge.end);
    }
    for ends in outgoing.values_mut() {
        ends.sort_unstable();
    }

    let mut unused: HashSet<Edge> = edges.iter().copied().collect();
    let mut contours = Vec::new();

    for edge in edges {
        if !unused.contains(&edge) {
            continue;
        }

        let contour = trace_loop(edge, &outgoing, &mut unused);
        if contour.len() >= 3 {
            contours.push(contour);
        }
    }

    contours.sort_by(|left, right| {
        contour_sort_key(left)
            .cmp(&contour_sort_key(right))
            .then_with(|| left.len().cmp(&right.len()))
    });
    contours
}

fn trace_loop(
    start_edge: Edge,
    outgoing: &HashMap<Point, Vec<Point>>,
    unused: &mut HashSet<Edge>,
) -> Contour {
    let mut contour = vec![start_edge.start];
    let mut current_edge = start_edge;

    loop {
        unused.remove(&current_edge);

        let current_point = current_edge.end;
        if current_point == start_edge.start {
            break;
        }

        contour.push(current_point);

        let Some(next_point) = choose_next_point(current_edge, outgoing, unused) else {
            break;
        };

        current_edge = Edge {
            start: current_point,
            end: next_point,
        };
    }

    contour
}

fn choose_next_point(
    current_edge: Edge,
    outgoing: &HashMap<Point, Vec<Point>>,
    unused: &HashSet<Edge>,
) -> Option<Point> {
    let current_direction = direction_from_edge(current_edge)?;
    let mut candidates: Vec<(u8, Point)> = outgoing
        .get(&current_edge.end)?
        .iter()
        .copied()
        .filter_map(|end| {
            let next_edge = Edge {
                start: current_edge.end,
                end,
            };
            if !unused.contains(&next_edge) {
                return None;
            }

            let direction = direction_from_edge(next_edge)?;
            Some((turn_priority(current_direction, direction), end))
        })
        .collect();

    candidates.sort_unstable_by_key(|(priority, point)| (*priority, *point));
    candidates.first().map(|(_, point)| *point)
}

fn direction_from_edge(edge: Edge) -> Option<Direction> {
    match (edge.end.x - edge.start.x, edge.end.y - edge.start.y) {
        (0, -1) => Some(Direction::Up),
        (1, 0) => Some(Direction::Right),
        (0, 1) => Some(Direction::Down),
        (-1, 0) => Some(Direction::Left),
        _ => None,
    }
}

fn turn_priority(current: Direction, next: Direction) -> u8 {
    match (next.index() - current.index()).rem_euclid(4) {
        1 => 0, // right turn
        0 => 1, // straight
        3 => 2, // left turn
        2 => 3, // reverse
        _ => unreachable!(),
    }
}

fn contour_sort_key(contour: &Contour) -> (i32, i32, i32, i32) {
    let min_x = contour
        .iter()
        .map(|point| point.x)
        .min()
        .unwrap_or_default();
    let min_y = contour
        .iter()
        .map(|point| point.y)
        .min()
        .unwrap_or_default();
    (min_y, min_x, contour[0].y, contour[0].x)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::segment::{PaletteColor, SegmentationResult};

    #[test]
    fn contour_rectangle() {
        let labels = vec![0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];
        let seg = SegmentationResult {
            palette: vec![
                PaletteColor { r: 255, g: 0, b: 0 },
                PaletteColor { r: 0, g: 255, b: 0 },
            ],
            labels,
            width: 4,
            height: 4,
        };

        let contours = extract_contours(&seg);
        assert_eq!(contours[0].1.len(), 1);
        assert!(signed_area(&contours[0].1[0]) > 0.0);
    }

    #[test]
    fn contour_ring_preserves_hole() {
        let labels = vec![
            1, 1, 1, 1, 1, 1, 1, //
            1, 0, 0, 0, 0, 0, 1, //
            1, 0, 1, 1, 1, 0, 1, //
            1, 0, 1, 1, 1, 0, 1, //
            1, 0, 1, 1, 1, 0, 1, //
            1, 0, 0, 0, 0, 0, 1, //
            1, 1, 1, 1, 1, 1, 1, //
        ];
        let seg = SegmentationResult {
            palette: vec![
                PaletteColor { r: 0, g: 0, b: 0 },
                PaletteColor {
                    r: 255,
                    g: 255,
                    b: 255,
                },
            ],
            labels,
            width: 7,
            height: 7,
        };

        let contours = extract_contours(&seg);
        let black_contours = &contours[0].1;
        assert_eq!(black_contours.len(), 2);
        assert_eq!(
            black_contours
                .iter()
                .filter(|contour| contour_is_hole(contour))
                .count(),
            1
        );
    }

    #[test]
    fn diagonal_pixels_trace_as_separate_loops() {
        let labels = vec![0, 1, 1, 0];
        let seg = SegmentationResult {
            palette: vec![
                PaletteColor { r: 0, g: 0, b: 0 },
                PaletteColor {
                    r: 255,
                    g: 255,
                    b: 255,
                },
            ],
            labels,
            width: 2,
            height: 2,
        };

        let contours = extract_contours(&seg);
        assert_eq!(contours[0].1.len(), 2);
    }
}
