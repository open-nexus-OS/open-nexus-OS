// src/desktop.rs
// Desktop start menu (small panel & large overlay) using the shared ui::draw_app_cell.
// Large mode respects the ActionBar top inset and leaves room for the bottom bar.
// Close-on-focus-loss is intentionally kept.

use orbclient::{Color, EventOption, Renderer, Window, WindowFlag, K_ESC, K_LEFT, K_RIGHT, K_UP, K_DOWN, K_BKSP, K_ENTER};
use orbimage::ResizeType;
use orbfont::Font;

use crate::config;
use crate::services::icon_service::CommonIcons;
use crate::config::settings::BAR_HEIGHT;
use crate::config::colors::{
    menu_surface_lg_paint, menu_surface_sm_paint, text_fg, text_inverse_fg, load_crisp_font,
};
use crate::utils::dpi_helper;
use libnexus::themes::Paint;

use crate::ui::layout; // shared draw_app_cell

#[cfg(target_os = "redox")]
const UI_PATH: &str = "/ui";
#[cfg(not(target_os = "redox"))]
const UI_PATH: &str = "ui";

pub enum DesktopMenuResult {
    None,
    Launch(String),
    Logout,
}

/// Rounded rect filler for translucent surfaces.
fn fill_round_rect(win: &mut Window, x: i32, y: i32, w: u32, h: u32, r: i32, color: Color) {
    let w_i = w as i32;
    let h_i = h as i32;
    if r <= 0 || w < (2 * r as u32) || h < (2 * r as u32) {
        win.rect(x, y, w, h, color);
        return;
    }
    for yi in 0..h_i {
        let dy = if yi < r {
            r - 1 - yi
        } else if yi >= h_i - r {
            yi - (h_i - r)
        } else {
            -1
        };
        let (sx, ex) = if dy >= 0 {
            let dx = ((r * r - dy * dy) as f32).sqrt().floor() as i32;
            (x + r - dx, x + w_i - r + dx)
        } else {
            (x, x + w_i)
        };
        let line_w = (ex - sx).max(0) as u32;
        if line_w > 0 {
            win.rect(sx, y + yi, line_w, 1, color);
        }
    }
}

/// Acrylic-like background placeholder (no blur capture yet).
fn render_acrylic_effect(win: &mut Window, x: i32, y: i32, w: u32, h: u32, r: i32, paint: Paint) {
    fill_round_rect(win, x, y, w, h, r, paint.color);
}

#[inline]
fn point_in(p: (i32, i32), r: (i32, i32, i32, i32)) -> bool {
    let (x, y) = p;
    let (rx, ry, rw, rh) = r;
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

pub fn show_desktop_menu(
    screen_w: u32,
    screen_h: u32,
    pkgs: &mut [crate::package::Package],
) -> DesktopMenuResult {
    let mut large = crate::config::settings::desktop_large();

    // Respect ActionBar top inset in large mode
    let top_inset = crate::config::settings::top_inset() as i32;

    let (mut win_x, mut win_y, mut win_w, mut win_h) = if large {
        // Large overlay: below top bar, above bottom bar
        (
            0,
            top_inset,
            screen_w,
            screen_h.saturating_sub(BAR_HEIGHT + crate::config::settings::top_inset())
        )
    } else {
        // Small panel: centered above bottom bar (unchanged)
        let w = (screen_w as f32 * 0.46) as u32;
        let h = (screen_h as f32 * 0.42) as u32;
        let x = ((screen_w - w) / 2) as i32;
        let y = (screen_h as i32) - (BAR_HEIGHT as i32) - 11 - (h as i32);
        (x, y, w, h)
    };

    let mut window = Window::new_flags(
        win_x, win_y, win_w, win_h, "StartMenuDesktop",
        &[WindowFlag::Async, WindowFlag::Borderless, WindowFlag::Transparent],
    ).expect("desktop menu window");

    let font = load_crisp_font();
    let icons = CommonIcons::load(UI_PATH);
    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());

    // UI state
    let mut query = String::new();
    let mut mouse_pos = (0, 0);
    let mut mouse_down = false;
    let mut last_mouse_down = false;

    // Paging & scrolling
    let mut page: usize = 0;     // large mode (pages)
    let mut list_top: usize = 0; // small mode (top row offset)

    // Hitboxes
    let mut power_hit:   (i32,i32,i32,i32) = (0, 0, 0, 0);
    let mut settings_hit:(i32,i32,i32,i32) = (0, 0, 0, 0);
    let mut search_rect: (i32,i32,i32,i32);
    let mut toggle_hit:  (i32,i32,i32,i32);
    let mut dot_hits: Vec<((i32,i32,i32,i32), usize)> = Vec::new();

    let mut search_active = false;

    'ev: loop {
        // Background
        if large {
            let paint = menu_surface_lg_paint();
            window.set(paint.color);
        } else {
            let paint = menu_surface_sm_paint();
            window.set(paint.color);
            let ww = window.width();
            let wh = window.height();
            render_acrylic_effect(&mut window, 0, 0, ww, wh, 5, paint);
        }

        // Toggle icon (top-right)
        let toggle_icon = if large { &icons.resize_lg } else { &icons.resize_sm };
        let tiw = toggle_icon.width() as i32;
        let tih = toggle_icon.height() as i32;
        let rx = window.width() as i32 - tiw - 10; // right inset
        let ry = 10;
        toggle_hit = (rx, ry, tiw, tih);

        // Search bar (top, truncated to not overlap toggle)
        let pad = 14i32;
        let search_h = 36i32;
        let sx = pad;
        let sy = pad;
        let sw = (rx - sx - 10).max(80);
        let sh = search_h;

        let search_bg = if large {
            if search_active { Color::rgba(255,255,255,34) } else { Color::rgba(255,255,255,26) }
        } else {
            if search_active { Color::rgba(0,0,0,28) } else { Color::rgba(0,0,0,18) }
        };
        fill_round_rect(&mut window, sx, sy, sw as u32, sh as u32, 6, search_bg);

        let qtxt = if query.is_empty() && !search_active { "Search apps…" } else { &query };
        let qcol = if large { text_inverse_fg() } else { text_fg() };
        let text = font.render(qtxt, dpi_helper::font_size(14.0).round());
        let text_x = sx + 10;
        let text_y = sy + (sh - text.height() as i32)/2;

        if large {
            // Slight “bold-ish” effect on dark background
            text.draw(&mut window, text_x, text_y, qcol);
            text.draw(&mut window, text_x, text_y, qcol);
        } else {
            text.draw(&mut window, text_x, text_y, qcol);
        }

        if search_active {
            let caret_x = (text_x + text.width() as i32).min(sx + sw - 8);
            let caret_top = sy + 6;
            let caret_h   = sh - 12;
            window.rect(caret_x, caret_top, 1, caret_h as u32, qcol);
        }
        search_rect = (sx, sy, sw, sh);

        // Toggle hover + draw
        if point_in(mouse_pos, toggle_hit) {
            fill_round_rect(
                &mut window,
                rx - 6,
                ry - 6,
                (tiw + 12) as u32,
                (tih + 12) as u32,
                6,
                if large { Color::rgba(255,255,255,28) } else { Color::rgba(0,0,0,22) },
            );
        }
        toggle_icon.draw(&mut window, rx, ry);

        // Filter packages
        let indices: Vec<usize> = if query.trim().is_empty() {
            (0..pkgs.len()).collect()
        } else {
            let ql = query.to_lowercase();
            pkgs
                .iter()
                .enumerate()
                .filter_map(|(i, p)| if p.name.to_lowercase().contains(&ql) { Some(i) } else { None })
                .collect()
        };

        // Content area
        let pad = 14i32;
        let base_content_x = pad;
        let base_content_y = sy + sh + 12;
        let base_content_w = window.width() as i32 - pad * 2;
        let bottom_reserve = if large { 96 } else { 64 };
        let base_content_h = (window.height() as i32 - bottom_reserve) - base_content_y;

        // Grid params
        let label_h = 16;
        let cell_pad = if large { 10 } else { 8 };
        let hgap = if large { 18 } else { 12 };
        let vgap = if large { 20 } else { 12 };
        let cols: i32 = 8;

        // Compute cell width and icon size
        let cell_w_avail = ((base_content_w - (cols - 1) * hgap) / cols).max(48);
        let icon_side_raw = (cell_w_avail - 2 * cell_pad).max(24);

        let icon_side = if large {
            (icon_side_raw as f32 * crate::dpi_scale()).max(64.0).min(96.0) as i32
        } else {
            (icon_side_raw as f32 * crate::dpi_scale()).max(32.0).min(48.0) as i32
        };
        let cell_w = icon_side + 2 * cell_pad;
        let cell_h = icon_side + label_h + 2 * cell_pad;

        // Center grid horizontally
        let grid_w = cols * cell_w + (cols - 1) * hgap;
        let grid_x = ((window.width() as i32 - grid_w) / 2).max(base_content_x);
        let grid_y = base_content_y;
        let content_h = base_content_h;

        // Rows
        let rows_avail = ((content_h + vgap) / (cell_h + vgap)).max(1);
        let rows: i32 = if large { rows_avail.min(5) } else { rows_avail };

        // Paging
        let per_page: usize = (cols * rows).max(1) as usize;
        let page_count = ((indices.len() + per_page - 1) / per_page).max(1);
        if large && page >= page_count {
            page = page_count - 1;
        }

        let visible_indices: Vec<usize> = if large {
            let start = page * per_page;
            let end = (start + per_page).min(indices.len());
            indices.get(start..end).unwrap_or(&[]).to_vec()
        } else {
            let total_rows = ((indices.len() as i32) + cols - 1) / cols;
            let max_top = (total_rows - rows).max(0) as usize;
            if list_top > max_top {
                list_top = max_top;
            }
            let start = (list_top as i32 * cols) as usize;
            let end = (start + per_page).min(indices.len());
            indices.get(start..end).unwrap_or(&[]).to_vec()
        };

        // Draw cells
        let mut cells: Vec<((i32, i32, i32, i32), usize)> = Vec::new();
        for (i, idx) in visible_indices.iter().enumerate() {
            let row = (i as i32) / cols;
            let col = (i as i32) % cols;
            let cx = grid_x + col * (cell_w + hgap);
            let cy = grid_y + row * (cell_h + vgap);

            // Hover veil
            if point_in(mouse_pos, (cx, cy, cell_w, cell_h)) {
                let veil = if large { Color::rgba(255, 255, 255, 28) } else { Color::rgba(0, 0, 0, 22) };
                fill_round_rect(&mut window, cx, cy, cell_w as u32, cell_h as u32, 8, veil);
            }

            let rect = crate::ui::components::draw_app_cell(
                &mut window,
                &font,
                &mut pkgs[*idx],
                cx,
                cy,
                cell_w,
                cell_h,
                icon_side,
                true,
                large,
            );
            cells.push((rect, *idx));
        }

        // Bottom controls
        let margin = 16i32;
        let gap = 16i32;

        // left: user avatar + name
        let target_h = if large { 22 } else { 20 };
        let mut user_img = icons.user.clone();
        let th_u = target_h as u32;
        let tw_u = ((user_img.width() * th_u) / user_img.height()).max(1);
        user_img = user_img.resize(tw_u, th_u, ResizeType::Lanczos3).unwrap();

        let user_x = margin;
        let user_y = window.height() as i32 - target_h - margin;

        let name_text = font.render(&username, dpi_helper::font_size(16.0).round());
        let name_x = user_x + user_img.width() as i32 + 8;
        let name_y = user_y + (target_h - name_text.height() as i32) / 2;
        let user_hit = (
            user_x,
            user_y.min(name_y),
            (name_x + name_text.width() as i32) - user_x,
            target_h.max(name_text.height() as i32),
        );

        if point_in(mouse_pos, user_hit) {
            let veil = if large { Color::rgba(255, 255, 255, 28) } else { Color::rgba(0, 0, 0, 22) };
            fill_round_rect(
                &mut window,
                user_hit.0 - 6,
                user_hit.1 - 6,
                (user_hit.2 + 12) as u32,
                (user_hit.3 + 12) as u32,
                6,
                veil,
            );
        }

        user_img.draw(&mut window, user_x, user_y);
        let base_name_color = if large { text_inverse_fg() } else { text_fg() };
        let name_col = if point_in(mouse_pos, user_hit) { base_name_color } else { base_name_color };
        if large {
            name_text.draw(&mut window, name_x, name_y, name_col);
            name_text.draw(&mut window, name_x, name_y, name_col);
        } else {
            name_text.draw(&mut window, name_x, name_y, name_col);
        }

        // right: settings + power
        let (settings_icon, power_icon) = if large {
            (&icons.settings_lg, &icons.power_lg)
        } else {
            (&icons.settings_sm, &icons.power_sm)
        };
        let sw2 = settings_icon.width() as i32;
        let sh2 = settings_icon.height() as i32;
        let pw = power_icon.width() as i32;
        let ph = power_icon.height() as i32;

        let settings_x = window.width() as i32 - sw2 - margin;
        let settings_y = window.height() as i32 - sh2 - margin;
        let power_x = settings_x - gap - pw;
        let power_y = window.height() as i32 - ph - margin;

        power_hit = (power_x, power_y, pw, ph);
        settings_hit = (settings_x, settings_y, sw2, sh2);

        // Hover effects
        if point_in(mouse_pos, power_hit) {
            let veil = if large { Color::rgba(255, 255, 255, 28) } else { Color::rgba(0, 0, 0, 22) };
            fill_round_rect(&mut window, power_x - 6, power_y - 6, (pw + 12) as u32, (ph + 12) as u32, 6, veil);
        }
        if point_in(mouse_pos, settings_hit) {
            let veil = if large { Color::rgba(255, 255, 255, 28) } else { Color::rgba(0, 0, 0, 22) };
            fill_round_rect(
                &mut window,
                settings_x - 6,
                settings_y - 6,
                (sw2 + 12) as u32,
                (sh2 + 12) as u32,
                6,
                veil,
            );
        }

        settings_icon.draw(&mut window, settings_x, settings_y);
        power_icon.draw(&mut window, power_x, power_y);

        // page dots (large only)
        dot_hits.clear();
        let total_pages = ((indices.len() + per_page - 1) / per_page).max(1);
        if large && total_pages > 1 {
            let dots_y = (window.height() as i32 - sh2 - margin) - 18;
            let dot_w = 8;
            let dot_h = 3;
            let spacing = 8;
            let total_w = (total_pages as i32) * dot_w + ((total_pages as i32 - 1) * spacing);
            let mut dx = (window.width() as i32 - total_w) / 2;
            for i in 0..total_pages {
                let active = i == page;
                let a = if active { 220 } else { 90 };
                dot_hits.push(((dx, dots_y, dot_w, dot_h), i));
                window.rect(dx, dots_y, dot_w as u32, dot_h as u32, Color::rgba(255, 255, 255, a));
                dx += dot_w + spacing;
            }
        }

        window.sync();

        // Events
        for ev in window.events() {
            match ev.to_option() {
                EventOption::Key(k) if k.scancode == K_ESC && k.pressed => break 'ev,
                EventOption::Key(k) if k.pressed => {
                    let ch = k.character;
                    let is_printable = ch != '\0' && !ch.is_control();

                    if k.scancode == K_BKSP {
                        if !search_active { search_active = true; }
                        let _ = query.pop();
                    } else if is_printable {
                        if !search_active { search_active = true; }
                        query.push(ch);
                    } else if k.scancode == K_ENTER && search_active {
                        // optional: launch first match
                    } else if !search_active {
                        match k.scancode {
                            K_LEFT if large => {
                                if page > 0 { page -= 1; }
                            }
                            K_RIGHT if large => {
                                if page + 1 < total_pages { page += 1; }
                            }
                            K_UP if !large => {
                                if list_top > 0 { list_top -= 1; }
                            }
                            K_DOWN if !large => {
                                let total_rows = ((indices.len() as i32) + cols - 1) / cols;
                                let max_top = (total_rows - rows).max(0) as usize;
                                if list_top < max_top { list_top += 1; }
                            }
                            _ => {}
                        }
                    }
                }
                EventOption::Mouse(m) => { mouse_pos = (m.x, m.y); }
                EventOption::Button(b) => {
                    mouse_down = b.left;
                    if b.left {
                        // Activate search when clicking inside search rect
                        if point_in(mouse_pos, search_rect) {
                            search_active = true;
                        } else {
                            search_active = false;
                        }
                    }
                }
                EventOption::Scroll(s) => {
                    if !large {
                        let dy = s.y;
                        let total_rows = ((indices.len() as i32) + cols - 1) / cols;
                        let max_top = (total_rows - rows).max(0) as usize;
                        if dy < 0 && (list_top as i32) < max_top as i32 {
                            list_top += 1;
                        } else if dy > 0 && list_top > 0 {
                            list_top -= 1;
                        }
                    }
                }
                // Keep close-on-focus-loss behavior as requested
                EventOption::Focus(f) => { if !f.focused { break 'ev; } }
                EventOption::Quit(_) => break 'ev,
                _ => {}
            }
        }

        // Mouse release edge
        if !mouse_down && last_mouse_down {
            if point_in(mouse_pos, toggle_hit) {
                // Toggle small/large, reset paging and icon caches, and reapply geometry
                large = !large;
                crate::config::settings::set_desktop_large(large);

                let top_inset = crate::config::settings::top_inset() as i32;
                (win_x, win_y, win_w, win_h) = if large {
                    (0, top_inset, screen_w, screen_h.saturating_sub(BAR_HEIGHT + crate::config::settings::top_inset()))
                } else {
                    let w = (screen_w as f32 * 0.46) as u32;
                    let h = (screen_h as f32 * 0.42) as u32;
                    let x = ((screen_w - w) / 2) as i32;
                    let y = (screen_h as i32) - (BAR_HEIGHT as i32) - 11 - (h as i32);
                    (x, y, w, h)
                };

                window.set_pos(win_x, win_y);
                window.set_size(win_w, win_h);
                page = 0;
                list_top = 0;

                for pkg in pkgs.iter_mut() {
                    pkg.clear_icon_caches();
                }
            } else if point_in(mouse_pos, settings_hit) {
                break 'ev;
            } else if point_in(mouse_pos, power_hit) {
                return DesktopMenuResult::Logout;
            } else {
                let mut launched = false;
                for (rect, idx) in &cells {
                    if point_in(mouse_pos, *rect) {
                        let exec = pkgs[*idx].exec.clone();
                        if !exec.trim().is_empty() {
                            return DesktopMenuResult::Launch(exec);
                        }
                        launched = true;
                        break;
                    }
                }
                if !launched {
                    if large && !point_in(mouse_pos, search_rect) {
                        // Background click closes large menu
                        break 'ev;
                    }
                }
            }
        }

        last_mouse_down = mouse_down;
    }

    DesktopMenuResult::None
}
