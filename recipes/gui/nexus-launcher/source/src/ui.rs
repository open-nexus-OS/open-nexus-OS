use orbclient::{Color, Renderer, Window};
use orbfont::Font;

use crate::themes::{TEXT_COLOR, TEXT_HIGHLIGHT_COLOR, BAR_HEIGHT};
use crate::package::Package;

pub struct SearchState { pub query: String }
impl Default for SearchState { fn default() -> Self { Self { query: String::new() } } }

pub struct GridLayout { pub cols: i32, pub cell: i32, pub gap: i32, pub top: i32 }

/// Compute grid layout depending on window width/height
pub fn compute_grid(width: u32, height: u32) -> GridLayout {
    let base = BAR_HEIGHT as i32;
    let gap = (base as f32 * 0.5) as i32;
    let cell = (base as f32 * 2.2) as i32;
    let cols = ((width as i32 - gap) / (cell + gap)).max(3);
    let top = (height as i32 / 6).min(120);
    GridLayout { cols, cell, gap, top }
}

/// Draws the searchbar (visual only, input handled externally)
pub fn draw_searchbar(w: &mut Window, font: &Font, y: i32, text: &str) -> (i32, i32, i32, i32) {
    let width = w.width() as i32;
    let bar_w = (width as f32 * 0.4).max(320.0) as i32;
    let bar_h = (BAR_HEIGHT as f32 * 0.9) as i32;
    let x = (width - bar_w) / 2;

    w.rect(x, y, bar_w as u32, bar_h as u32, Color::rgba(0xFF, 0xFF, 0xFF, 140));
    let label = if text.is_empty() { "  Search" } else { text };
    let rend = font.render(label, (bar_h as f32 * 0.5).max(14.0));
    let tx = x + 12;
    let ty = y + (bar_h - rend.height() as i32) / 2;
    rend.draw(w, tx, ty, TEXT_COLOR);

    (x, y, bar_w, bar_h)
}

/// Filter packages based on query (case-insensitive)
pub fn filter_packages<'a>(pkgs: &'a [Package], query: &str) -> Vec<usize> {
    if query.trim().is_empty() { return (0..pkgs.len()).collect(); }
    let q = query.to_lowercase();
    pkgs.iter().enumerate().filter_map(|(i,p)| {
        if p.name.to_lowercase().contains(&q) { Some(i) } else { None }
    }).collect()
}

/// Draw an app cell (icon + optional label)
pub fn draw_app_cell(win: &mut Window, font: &Font, x: i32, y: i32, size: i32, pkg: &mut Package, show_label: bool) {
    let img = pkg.icon.image();
    let ix = x + (size - img.width() as i32) / 2;
    let iy = y + (size - img.height() as i32) / 2;
    img.draw(win, ix, iy);

    if show_label {
        let text = font.render(&pkg.name, (size as f32 * 0.22).max(12.0));
        let tx = x + (size - text.width() as i32) / 2;
        let ty = y + size + 6;
        text.draw(win, tx, ty, TEXT_HIGHLIGHT_COLOR);
    }
}

/// Iterate grid, draw icons, return exec string if clicked
pub fn grid_iter_and_hit(
    win: &mut Window,
    font: &Font,
    layout: &GridLayout,
    pkgs: &mut [Package],
    indices: &[usize],
    mouse: Option<(i32,i32,bool,bool)>, // (x,y,down,released)
    show_labels: bool,
) -> Option<String> {
    let mut x = layout.gap;
    let mut y = layout.top + layout.gap + 44;
    let icon = (layout.cell as f32 * 0.78) as i32;

    let mut col = 0;
    for &idx in indices {
        let hit_x = x; let hit_y = y; let hit_w = layout.cell; let hit_h = layout.cell;

        draw_app_cell(win, font, x + (layout.cell - icon) / 2, y + (layout.cell - icon) / 2, icon, &mut pkgs[idx], show_labels);

        if let Some((mx,my,_down,released)) = mouse {
            if released && mx >= hit_x && mx < hit_x + hit_w && my >= hit_y && my < hit_y + hit_h {
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