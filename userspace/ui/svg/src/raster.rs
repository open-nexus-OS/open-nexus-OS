// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::vec::Vec;

use crate::elements::SvgDocument;
use crate::limits::OUTPUT_BYTES_PER_PIXEL;
use crate::tessellate::{tessellate_document, Edge};

/// Rasterized BGRA8888 output.
#[derive(Debug, Clone)]
pub struct RasterOutput {
    pub width: u32,
    pub height: u32,
    pub buffer: Vec<u8>,
}

/// Rasterize an SVG document to a BGRA8888 buffer.
pub fn rasterize_document(doc: &SvgDocument) -> Result<RasterOutput, crate::error::SvgError> {
    let width = (doc.width + 0.99999_f32) as u32;
    let height = (doc.height + 0.99999_f32) as u32;

    if width == 0 || height == 0 {
        return Ok(RasterOutput {
            width,
            height,
            buffer: Vec::new(),
        });
    }

    let edges = tessellate_document(doc);

    let size = (width as usize) * (height as usize) * OUTPUT_BYTES_PER_PIXEL;
    let mut buffer = vec![0u8; size];

    scanline_fill(&edges, width as usize, height as usize, &mut buffer);

    Ok(RasterOutput {
        width,
        height,
        buffer,
    })
}

/// Simple scanline polygon fill with alpha blending.
fn scanline_fill(edges: &[Edge], w: usize, h: usize, buffer: &mut [u8]) {
    if edges.is_empty() {
        return;
    }

    // Find y-range
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;
    for e in edges {
        min_y = min_y.min(e.y0);
        max_y = max_y.max(e.y1);
    }

    let y_start = (min_y as isize).max(0) as usize;
    let y_end = ((max_y as isize) + 1).min(h as isize - 1) as usize;

    // Sort edges by y0 for active edge management
    let mut sorted_edges: Vec<&Edge> = edges.iter().collect();
    sorted_edges.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap());

    let mut edge_idx = 0;
    let mut active: Vec<&Edge> = Vec::new();

    for y in y_start..=y_end {
        let yf = y as f32 + 0.5; // sample at pixel center

        // Remove edges that have ended
        active.retain(|e| e.y1 > yf);

        // Add new edges starting at this scanline
        while edge_idx < sorted_edges.len() && sorted_edges[edge_idx].y0 <= yf + 1.0 {
            active.push(sorted_edges[edge_idx]);
            edge_idx += 1;
        }

        if active.is_empty() {
            continue;
        }

        // Compute x-intersections
        let mut intersections: Vec<(f32, &Edge)> = Vec::new();
        for e in &active {
            let t = if (e.y1 - e.y0).abs() < 0.0001 {
                0.0
            } else {
                (yf - e.y0) / (e.y1 - e.y0)
            };
            let x = e.x0 + t * (e.x1 - e.x0);
            intersections.push((x, e));
        }

        intersections.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        // Fill between pairs (even-odd rule)
        for i in (0..intersections.len()).step_by(2) {
            if i + 1 >= intersections.len() {
                break;
            }
            let x_start = intersections[i].0.clamp(0.0, w as f32) as usize;
            let x_end = (intersections[i + 1].0.clamp(0.0, w as f32)) as usize;

            // Use the color of the first edge in the pair (simplification:
            // all edges in a polygon share the same color)
            let color = intersections[i].1.color;

            for x in x_start..x_end.min(w) {
                let idx = (y * w + x) * OUTPUT_BYTES_PER_PIXEL;
                blend_pixel(&mut buffer[idx..idx + 4], color);
            }
        }
    }
}

/// Alpha-blend a color onto a BGRA8888 pixel (premultiplied alpha).
fn blend_pixel(dst: &mut [u8], src_color: crate::elements::Color) {
    let sa = src_color.a as u32;
    if sa == 0 {
        return;
    }
    if sa == 255 {
        dst[0] = src_color.b;
        dst[1] = src_color.g;
        dst[2] = src_color.r;
        dst[3] = 255;
        return;
    }

    let da = dst[3] as u32;
    let inv_sa = 255 - sa;

    // Blend with premultiplied alpha
    dst[0] = ((src_color.b as u32 * sa + dst[0] as u32 * inv_sa * da / 255) / 255) as u8;
    dst[1] = ((src_color.g as u32 * sa + dst[1] as u32 * inv_sa * da / 255) / 255) as u8;
    dst[2] = ((src_color.r as u32 * sa + dst[2] as u32 * inv_sa * da / 255) / 255) as u8;
    dst[3] = ((sa * 255 + da * inv_sa) / 255) as u8;
}
