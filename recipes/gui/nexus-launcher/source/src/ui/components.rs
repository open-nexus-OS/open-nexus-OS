// ui/components.rs - UI components like app cells

use orbclient::{Color, Renderer, Window};
use orbfont::Font;
use orbimage::Image;

use crate::config::colors::{text_inverse_fg, text_fg};
use crate::utils::dpi_helper;
use crate::services::package_service::Package;

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
