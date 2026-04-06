//! Contour tracing using Moore neighbor contour following.
//!
//! For each color region (label), extract the outer boundary as a sequence
//! of pixel coordinates forming closed polygons.

use super::segment::SegmentationResult;

/// A 2D integer point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

/// Extract contours for all color regions.
///
/// Returns a list of `(palette_index, contours)` pairs.
pub fn extract_contours(seg: &SegmentationResult) -> Vec<(u8, Vec<Contour>)> {
    let w = seg.width as usize;
    let h = seg.height as usize;

    let num_colors = seg.palette.len();
    let mut result: Vec<(u8, Vec<Contour>)> = Vec::new();

    for color_idx in 0..num_colors as u8 {
        let contours = trace_color_contours(&seg.labels, w, h, color_idx);
        if !contours.is_empty() {
            result.push((color_idx, contours));
        }
    }

    result
}

/// Trace all contours for a single color label using a square-tracing algorithm.
fn trace_color_contours(labels: &[u8], width: usize, height: usize, target: u8) -> Vec<Contour> {
    // Create a binary mask for the target color
    let mask: Vec<bool> = labels.iter().map(|&l| l == target).collect();

    // Track which boundary pixels have been visited
    let mut visited = vec![false; width * height];
    let mut contours = Vec::new();

    for start_y in 0..height {
        for start_x in 0..width {
            let idx = start_y * width + start_x;
            if !mask[idx] || visited[idx] {
                continue;
            }
            // Check if this pixel is a boundary pixel
            if !is_boundary(&mask, start_x, start_y, width, height) {
                continue;
            }

            // Trace the contour starting from this pixel
            if let Some(contour) =
                trace_single_contour(&mask, &mut visited, start_x, start_y, width, height)
            {
                if contour.len() >= 3 {
                    contours.push(contour);
                }
            }
        }
    }

    contours
}

/// Check if a pixel is on the boundary of a color region.
fn is_boundary(mask: &[bool], x: usize, y: usize, width: usize, height: usize) -> bool {
    if !mask[y * width + x] {
        return false;
    }
    // A pixel is on the boundary if any of its 4-neighbors is outside the region
    let neighbors = [
        (x.wrapping_sub(1), y),
        (x + 1, y),
        (x, y.wrapping_sub(1)),
        (x, y + 1),
    ];
    neighbors
        .iter()
        .any(|&(nx, ny)| nx >= width || ny >= height || !mask[ny * width + nx])
}

/// Trace a single contour using Moore neighbor tracing (Jacob's stopping criterion).
fn trace_single_contour(
    mask: &[bool],
    visited: &mut [bool],
    start_x: usize,
    start_y: usize,
    width: usize,
    height: usize,
) -> Option<Contour> {
    // 8-connectivity Moore neighbors in order: E, SE, S, SW, W, NW, N, NE
    const DIRS: [(i32, i32); 8] = [
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
    ];

    let mut contour = Vec::new();
    let start = Point::new(start_x as i32, start_y as i32);

    let mut current = start;
    let mut dir = 7usize; // Start checking from NE direction

    let max_iter = width * height + 10;
    let mut iter_count = 0;

    loop {
        contour.push(current);
        visited[(current.y as usize) * width + (current.x as usize)] = true;

        // Find the next boundary pixel
        let mut found = false;
        for i in 0..8 {
            let d = (dir + i) % 8;
            let nx = current.x + DIRS[d].0;
            let ny = current.y + DIRS[d].1;

            if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                continue;
            }

            if mask[ny as usize * width + nx as usize] {
                // Rotate direction: the new "back" direction is the opposite of d
                dir = (d + 5) % 8;
                current = Point::new(nx, ny);
                found = true;
                break;
            }
        }

        if !found {
            break; // isolated pixel
        }

        // Jacob's stopping criterion: return to start
        if current == start && contour.len() > 1 {
            break;
        }

        iter_count += 1;
        if iter_count > max_iter {
            break; // safety valve
        }
    }

    if contour.is_empty() {
        None
    } else {
        Some(contour)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::segment::{PaletteColor, SegmentationResult};

    #[test]
    fn contour_rectangle() {
        // 4x4 image: label 0 = 2x2 block in top-left
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
        assert!(!contours.is_empty());
    }
}
