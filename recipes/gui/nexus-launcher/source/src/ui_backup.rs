// src/ui.rs
// Shared UI helpers + a single, canonical draw_app_cell used by desktop & mobile.

use orbclient::{Color, Renderer, Window};
use orbfont::Font;
use orbimage::Image;

use crate::config::{BAR_HEIGHT, text_inverse_fg, text_fg};
use crate::helper::dpi_helper;
use crate::package::Package;

pub struct SearchState { pub query: String }
impl Default for SearchState { fn default() -> Self { Self { query: String::new() } } }

pub struct GridLayout { pub cols: i32, pub cell: i32, pub gap: i32, pub top: i32 }

/// Compute a simple grid layout based on window size.
pub fn compute_grid(width: u32, height: u32) -> GridLayout {
    let base = BAR_HEIGHT as i32;
    let gap = (base as f32 * 0.5) as i32;
    let cell = (base as f32 * 2.2) as i32;
    let cols = ((width as i32 - gap) / (cell + gap)).max(3);
    let top = (height as i32 / 6).min(120);
    GridLayout { cols, cell, gap, top }
}

/// Canonical app cell renderer used everywhere.
/// - `icon_px`: desired icon side length (before DPI scaling)
/// - `large`: affects which package icon bucket is used and label color
/// Returns the clickable rect (x, y, w, h).
pub fn draw_app_cell(
    win: &mut Window,
    font: &Font,
    pkg: &mut Package,
    x: i32,
    y: i32,
    cell_w: i32,
    cell_h: i32,
    icon_px: i32,
    show_label: bool,
    large: bool,
) -> (i32, i32, i32, i32) {
    let pad = 8;
    let gap = 6;
    let dpi = crate::dpi_scale();

    // Copy name first to avoid immutable borrow after &mut call below.
    let name_owned = pkg.name.clone();

    // Get a crisp themed icon at requested (DPI-scaled) size.
    let icon = pkg.get_icon_sized(icon_px as u32, dpi, large);

    let ix = x + (cell_w - icon.width() as i32) / 2;
    let iy = y + pad;
    icon.draw(win, ix, iy);

    if show_label {
        let label_size = if large { 16.0 } else { 14.0 };
        let text = font.render(&name_owned, label_size);
        let tx = x + (cell_w - text.width() as i32) / 2;
        let ty = iy + icon.height() as i32 + gap;
        // Light text on dark (large), darker on light (small)
        let col = if large { Color::rgba(0xFF, 0xFF, 0xFF, 255) } else { Color::rgba(0x0A, 0x0A, 0x0A, 255) };

        if large {
            // TODO: fix for this workaround
            // Triple Layer for sharper text
            text.draw(win, tx, ty, col);
            text.draw(win, tx, ty, col);
        } else {
            text.draw(win, tx, ty, col);
        }
    }

    (x, y, cell_w, cell_h)
}

/// Iterate a grid and draw items; returns exec of the clicked package if any.
pub fn grid_iter_and_hit(
    win: &mut Window,
    font: &Font,
    layout: &GridLayout,
    pkgs: &mut [Package],
    indices: &[usize],
    mouse: Option<(i32,i32,bool,bool)>, // (x,y,down,released)
    show_labels: bool,
    large: bool,
) -> Option<String> {
    let mut x = layout.gap;
    let mut y = layout.top + layout.gap + 44;
    let icon = (layout.cell as f32 * if large { 0.82 } else { 0.78 }) as i32;

    let mut col = 0;
    for &idx in indices {
        let rect = draw_app_cell(
            win, font, &mut pkgs[idx],
            x, y, layout.cell, layout.cell,
            icon, show_labels, large,
        );

        if let Some((mx,my,_down,released)) = mouse {
            if released
                && mx >= rect.0 && mx < rect.0 + rect.2
                && my >= rect.1 && my < rect.1 + rect.3
            {
                let exec = pkgs[idx].exec.clone();
                if !exec.trim().is_empty() { return Some(exec); }
            }
        }

        col += 1;
        if col >= layout.cols {
            col = 0; x = layout.gap; y += layout.cell + layout.gap;
        } else {
            x += layout.cell + layout.gap;
        }
    }
    None
}
