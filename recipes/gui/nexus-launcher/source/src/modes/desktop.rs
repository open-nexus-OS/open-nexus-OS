use orbclient::{Color, EventOption, Renderer, Window, WindowFlag, K_ESC, K_LEFT, K_RIGHT, K_UP, K_DOWN};
use orbimage::{Image, ResizeType};
use orbfont::Font;

use crate::config;
use crate::icons::CommonIcons;
use crate::themes::{BAR_HEIGHT, BAR_COLOR};

use std::collections::HashMap;

#[cfg(target_os = "redox")]
const UI_PATH: &str = "/ui";
#[cfg(not(target_os = "redox"))]
const UI_PATH: &str = "ui";

pub enum DesktopMenuResult {
    None,
    Launch(String),
    Logout,
}

/// Simple rounded-rect fill for transparent windows
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

/// Subtle hover veil (light on dark; dark on light)
#[inline]
fn hover_fill_color(large: bool) -> Color {
    if large { Color::rgba(255, 255, 255, 28) } else { Color::rgba(0, 0, 0, 22) }
}

/// Compute geometry for small/large desktop menus.
/// - small: horizontally centered, y = 11 px above taskbar
/// - large: fullscreen overlay except taskbar height
fn target_geometry(screen_w: u32, screen_h: u32, large: bool) -> (i32, i32, u32, u32) {
    if large {
        (0, 0, screen_w, screen_h.saturating_sub(BAR_HEIGHT))
    } else {
        let w = (screen_w as f32 * 0.46) as u32;
        let h = (screen_h as f32 * 0.42) as u32;
        let x = ((screen_w - w) / 2) as i32;
        let y = (screen_h as i32) - (BAR_HEIGHT as i32) - 11 - (h as i32);
        (x, y, w, h)
    }
}

#[inline]
fn point_in(p: (i32, i32), r: (i32, i32, i32, i32)) -> bool {
    let (x, y) = p;
    let (rx, ry, rw, rh) = r;
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

/// Draw exactly one app cell (icon + label) inside the given rectangle.
/// - Uses a pre-resized icon (`pre_icon`) if provided (fast path, from cache)
/// - Otherwise resizes on the fly to `icon_side` (slow path)
/// - Returns the clickable rect of this cell: (x, y, w, h)
pub fn draw_app_cell(
    win: &mut Window,
    font: &Font,
    pkg: &mut crate::package::Package,
    x: i32,
    y: i32,
    cell_w: i32,
    cell_h: i32,
    icon_side: i32,
    large: bool,
    pre_icon: Option<&Image>,
) -> (i32, i32, i32, i32) {
    // Layout constants
    let pad = 8;           // inner padding at top/left/right
    let label_gap = 6;     // gap between icon and label
    let label_size = 14.0; // font size for app name

    // Use the cached, pre-resized icon if available; otherwise resize now
    let mut owned_icon: Option<Image> = None;
    let icon_ref: &Image = if let Some(img) = pre_icon {
        img
    } else {
        // Fallback: compute resized icon now (slower, but okay as a fallback)
        let base = pkg.icon.image().clone();
        let img = base
            .resize(icon_side as u32, icon_side as u32, ResizeType::Lanczos3)
            .expect("icon resize failed");
        owned_icon = Some(img);
        owned_icon.as_ref().unwrap()
    };

    // Place the icon centered horizontally, near the top with padding
    let ix = x + (cell_w - icon_ref.width() as i32) / 2;
    let iy = y + pad;

    icon_ref.draw(win, ix, iy);

    // Render the label below the icon, centered horizontally
    let label = font.render(&pkg.name, label_size);
    let tx = x + (cell_w - label.width() as i32) / 2;
    let ty = iy + icon_ref.height() as i32 + label_gap;

    // Use light text on dark (large/fullscreen), dark text on light (small panel)
    let text_color = if large {
        Color::rgba(0xFF, 0xFF, 0xFF, 240)
    } else {
        Color::rgba(0x14, 0x14, 0x14, 240)
    };
    label.draw(win, tx, ty, text_color);

    // Return the whole cell rect as the clickable area
    (x, y, cell_w, cell_h)
}

pub fn show_desktop_menu(screen_w: u32, screen_h: u32, pkgs: &mut [crate::package::Package]) -> DesktopMenuResult {

    let mut icon_cache: HashMap<(usize, i32), Image> = HashMap::new();

    let mut large = config::desktop_large();
    let (mut win_x, mut win_y, mut win_w, mut win_h) = target_geometry(screen_w, screen_h, large);
    let mut window = Window::new_flags(
        win_x, win_y, win_w, win_h, "StartMenuDesktop",
        &[WindowFlag::Async, WindowFlag::Borderless, WindowFlag::Transparent],
    ).expect("desktop menu window");

    let font = Font::find(Some("Sans"), None, None).unwrap();
    let icons = CommonIcons::load(UI_PATH);
    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());

    let mut query = String::new();
    let mut mouse_pos = (0, 0);
    let mut mouse_down = false;
    let mut last_mouse_down = false;

    // Paging & scrolling state
    let mut page: usize = 0;         // large mode & mobile use pages
    let mut list_top: usize = 0;     // small mode vertical list top index

    // Hitboxes
    let mut power_hit: (i32,i32,i32,i32);
    let mut settings_hit: (i32,i32,i32,i32);
    let mut search_rect: (i32,i32,i32,i32);
    let mut toggle_hit: (i32,i32,i32,i32);
    let mut dot_hits: Vec<((i32,i32,i32,i32), usize)> = Vec::new();

    'ev: loop {
        // Backgrounds
        if large {
            window.set(Color::rgba(0, 0, 0, 80)); // dark overlay
        } else {
            window.set(Color::rgba(0, 0, 0, 0));  // clear fully transparent
            let ww = window.width();
            let wh = window.height();
            fill_round_rect(&mut window, 0, 0, ww, wh, 5, Color::rgba(255, 255, 255, 210)); // bright rounded panel
        }

        // --- Search bar (top) ---
        // Simple inline search bar: 14px font, returns (x,y,w,h)
        let pad = 14i32;
        let search_h = 36i32;
        let sx = pad;
        let sy = pad;
        let sw = window.width() as i32 - pad*2;
        let sh = search_h;

        // Search background (rounded) to match style
        fill_round_rect(&mut window, sx, sy, sw as u32, sh as u32, 6,
            if large { Color::rgba(255,255,255,26) } else { Color::rgba(0,0,0,18) });
        // Search text
        let qtxt = if query.is_empty() { "Search apps…" } else { &query };
        let qcol = if large {
            if query.is_empty() { Color::rgba(255,255,255,150) } else { Color::rgba(255,255,255,230) }
        } else {
            if query.is_empty() { Color::rgba(20,20,20,160) } else { Color::rgba(20,20,20,230) }
        };
        let q = font.render(qtxt, 14.0);
        q.draw(&mut window, sx + 10, sy + (sh - q.height() as i32)/2, qcol);

        search_rect = (sx, sy, sw, sh);

        // --- Filter packages by query ---
        let mut indices: Vec<usize> = Vec::with_capacity(pkgs.len());
        if query.trim().is_empty() {
            indices.extend(0..pkgs.len());
        } else {
            let ql = query.to_lowercase();
            for (i, p) in pkgs.iter().enumerate() {
                if p.name.to_lowercase().contains(&ql) {
                    indices.push(i);
                }
            }
        }

        // --- Toggle icon (top-right) ---
        let toggle_icon = if large { &icons.resize_lg } else { &icons.resize_sm };
        let tiw = toggle_icon.width() as i32;
        let tih = toggle_icon.height() as i32;
        let rx = window.width() as i32 - tiw - 10;
        let ry = 10;
        toggle_hit = (rx, ry, tiw, tih);
        if point_in(mouse_pos, toggle_hit) {
            let hp = 6;
            fill_round_rect(&mut window, rx - hp, ry - hp, (tiw + 2*hp) as u32, (tih + 2*hp) as u32, 6, hover_fill_color(large));
        }
        toggle_icon.draw(&mut window, rx, ry);

        // --- Content area and grid/list layout ---
        let content_x = pad;
        let content_y = sy + sh + 12;
        let content_w = window.width() as i32 - pad*2;

        // Bottom area reserved for user/settings/power + page dots spacing
        let bottom_reserve = 64i32;

        let content_h = (window.height() as i32 - bottom_reserve) - content_y;

        // Layout params
        let (cols, rows) = if large { (8usize, 5usize) } else { (1usize, (content_h / 88).max(3) as usize) };
        let hgap = if large { 18 } else { 8 };
        let vgap = if large { 18 } else { 8 };

        // Compute cell size
        let cell_w = ((content_w - (cols as i32 - 1) * hgap) / cols as i32).max(48);
        let cell_h = if large {
            ((content_h - (rows as i32 - 1) * vgap) / rows as i32).max(64)
        } else {
            // small: one column, dynamic height ~ 80-100
            ((content_h - (rows as i32 - 1) * vgap) / rows as i32).clamp(64, 112)
        };

        // Paging / list windowing
        let per_page = cols * rows;
        let page_count = if per_page == 0 { 1 } else { ((indices.len() + per_page - 1) / per_page).max(1) };
        if large && page >= page_count { page = page_count - 1; }

        // --- Visible slice ---
        let visible_indices: Vec<usize> = if large {
            let start = page * per_page;
            let end = (start + per_page).min(indices.len());
            indices.get(start..end).unwrap_or(&[]).to_vec()
        } else {
            // small: 1 column with vertical list
            let visible_rows = rows.max(1);
            // clamp list_top so that at least one full "page" (rows) sichtbar ist
            let max_top = indices.len().saturating_sub(visible_rows);
            if list_top > max_top { list_top = max_top; }

            let end = (list_top + visible_rows).min(indices.len());
            indices.get(list_top..end).unwrap_or(&[]).to_vec()
        };

        // Draw grid/list and collect hit rects
        let mut cells: Vec<((i32,i32,i32,i32), usize)> = Vec::new();
        let mut cx = content_x;
        let mut cy = content_y;

        for (i, idx) in visible_indices.iter().enumerate() {
            let row = (i / cols) as i32;
            let col = (i % cols) as i32;

            cx = content_x + col * (cell_w + hgap);
            cy = content_y + row * (cell_h + vgap);

            // --- compute the same icon size we use for caching ---
            let label_h = 16i32;
            let pad = 8i32;
            let usable_h = (cell_h - label_h - pad*2).max(24);
            let usable_w = (cell_w - pad*2).max(24);
            let icon_side = usable_h.min(usable_w).max(28);

            // Hover background (unchanged)
            if point_in(mouse_pos, (cx, cy, cell_w, cell_h)) {
                fill_round_rect(&mut window, cx, cy, cell_w as u32, cell_h as u32, 8, hover_fill_color(large));
            }

            // --- ICON CACHE LOOKUP (this is the piece you asked for) ---
            let key = (*idx, icon_side);
            let icon_ref = if let Some(img) = icon_cache.get(&key) {
                img
            } else {
                let img_src = pkgs[*idx].icon.image().clone();
                let img = img_src
                    .resize(icon_side as u32, icon_side as u32, ResizeType::Lanczos3)
                    .expect("resize");
                icon_cache.insert(key, img);
                icon_cache.get(&key).unwrap()
            };

            // Draw cell using the pre-resized icon
            let rect = draw_app_cell(
                &mut window,
                &font,
                &mut pkgs[*idx],
                cx, cy,
                cell_w, cell_h,
                icon_side,
                large,
                Some(icon_ref),
            );

            cells.push((rect, *idx));
        }

        // --- Bottom controls ---
        let margin = 16i32;
        let gap = 16i32;

        // Avatar + username (left)
        let target_h = if large { 22 } else { 20 };
        let mut user_img = icons.user.clone();
        let target_h_u = target_h as u32;
        let target_w_u = ((user_img.width() * target_h_u) / user_img.height()).max(1);
        user_img = user_img.resize(target_w_u, target_h_u, ResizeType::Lanczos3).unwrap();

        let user_x = margin;
        let user_y = window.height() as i32 - target_h - margin;

        // Hover user area
        let name_text = font.render(&username, 16.0);
        let name_x = user_x + user_img.width() as i32 + 8;
        let name_y = user_y + (target_h - name_text.height() as i32) / 2;
        let user_hit = (user_x, user_y.min(name_y), (name_x + name_text.width() as i32) - user_x, target_h.max(name_text.height() as i32));
        if point_in(mouse_pos, user_hit) {
            fill_round_rect(&mut window, user_hit.0 - 6, user_hit.1 - 6, (user_hit.2 + 12) as u32, (user_hit.3 + 12) as u32, 6, hover_fill_color(large));
        }

        user_img.draw(&mut window, user_x, user_y);
        let base_name_color = if large { Color::rgba(0xFF, 0xFF, 0xFF, 230) } else { Color::rgba(0x14, 0x14, 0x14, 220) };
        let name_col = if point_in(mouse_pos, user_hit) {
            if large { Color::rgba(0xFF, 0xFF, 0xFF, 255) } else { Color::rgba(0x14, 0x14, 0x14, 255) }
        } else { base_name_color };
        name_text.draw(&mut window, name_x, name_y, name_col);

        // Right: settings (outer) + power (left)
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

        settings_hit = (settings_x, settings_y, sw2, sh2);
        power_hit    = (power_x,    power_y,    pw,  ph);

        // Hover backgrounds
        if point_in(mouse_pos, settings_hit) {
            fill_round_rect(&mut window, settings_x - 6, settings_y - 6, (sw2 + 12) as u32, (sh2 + 12) as u32, 6, hover_fill_color(large));
        }
        if point_in(mouse_pos, power_hit) {
            fill_round_rect(&mut window, power_x - 6, power_y - 6, (pw + 12) as u32, (ph + 12) as u32, 6, hover_fill_color(large));
        }

        // Draw icons last
        settings_icon.draw(&mut window, settings_x, settings_y);
        power_icon.draw(&mut window, power_x, power_y);

        // --- Divider line (small only): from user_x to settings right edge ---
        if !large {
            let controls_top = user_y.min(settings_y).min(power_y);
            let sep_y = (controls_top - 8).max(0);
            let sep_x1 = user_x;
            let sep_x2 = settings_x + sw2;
            if sep_x2 > sep_x1 {
                window.rect(sep_x1, sep_y, (sep_x2 - sep_x1) as u32, 1, Color::rgba(255, 255, 255, 230));
            }
        }

        // --- Page dots (large only) ---
        if large && page_count > 1 {
            let dots_y = (window.height() as i32 - sh2 - margin) - 18;
            let dot_w = 8;
            let dot_h = 3;
            let spacing = 8;
            let total_w = (page_count as i32) * dot_w + ((page_count as i32 - 1) * spacing);
            let mut dx = (window.width() as i32 - total_w) / 2;
            for i in 0..page_count {
                let active = i == page;
                let a = if active { 220 } else { 90 };
                // store hit rect
                dot_hits.push(((dx, dots_y, dot_w, dot_h), i));
                window.rect(dx, dots_y, dot_w as u32, dot_h as u32, Color::rgba(255,255,255,a));
                dx += dot_w + spacing;
            }
        }

        window.sync();

        // --- Events ---
        for ev in window.events() {
            match ev.to_option() {
                EventOption::Key(k) if k.scancode == K_ESC && k.pressed => break 'ev,
                EventOption::Key(k) if k.pressed => {
                    let ch = k.character;
                    // type into search
                    match ch {
                        '\u{8}' => { query.pop(); }
                        '\n' | '\r' => {}
                        c if c != '\0' && !c.is_control() => query.push(c),
                        _ => {}
                    }
                    // navigation keys
                    match k.scancode {
                        K_LEFT  if large => { if page > 0 { page -= 1; } }
                        K_RIGHT if large => { if page + 1 < page_count { page += 1; } }
                        K_UP    if !large => { if list_top > 0 { list_top -= 1; } }
                        K_DOWN  if !large => {
                            if indices.len() > 0 {
                                let max_top = indices.len().saturating_sub(rows.max(1));
                                if list_top < max_top { list_top += 1; }
                            }
                        }
                        _ => {}
                    }
                }
                EventOption::Mouse(m) => { mouse_pos = (m.x, m.y); }
                EventOption::Button(b) => { mouse_down = b.left; }
                EventOption::Scroll(s) => {
                    // NOTE: On orbclient, positive y typically means "scroll up", negative "down".
                    if !large {
                        // one step per event
                        let visible_rows = rows.max(1);
                        let max_top = indices.len().saturating_sub(visible_rows);

                        // Try to read vertical delta; if your orbclient uses a different field
                        // name (e.g. dy), change 's.y' to match. We only need the sign.
                        if s.y < 0 {
                            // scroll down
                            if list_top < max_top { list_top = list_top.saturating_add(1); }
                        } else if s.y > 0 {
                            // scroll up
                            if list_top > 0 { list_top -= 1; }
                        }
                    }
                }
                EventOption::Focus(f) => { if !f.focused { break 'ev; } }
                EventOption::Quit(_) => break 'ev,
                _ => {}
            }
        }

        // --- Release edge handling ---
        if !mouse_down && last_mouse_down {
            // 1) Toggle small/large
            if point_in(mouse_pos, toggle_hit) {
                large = !large;
                config::set_desktop_large(large);
                (win_x, win_y, win_w, win_h) = target_geometry(screen_w, screen_h, large);
                window.set_pos(win_x, win_y);
                window.set_size(win_w, win_h);
                // reset paging/scroll
                page = 0;
                list_top = 0;
            }
            // 2) Settings closes menu (MVP)
            else if point_in(mouse_pos, settings_hit) {
                break 'ev;
            }
            // 3) Power → Logout
            else if point_in(mouse_pos, power_hit) {
                return DesktopMenuResult::Logout;
            }
            // 4) App cells (both modes): launch if a cell was clicked
            else {
                let mut launched = false;
                for (rect, idx) in &cells {
                    if point_in(mouse_pos, *rect) {
                        let exec = pkgs[*idx].exec.clone();
                        if !exec.trim().is_empty() {
                            return DesktopMenuResult::Launch(exec);
                        }
                        launched = true; // clicked but empty exec; treated as handled
                        break;
                    }
                }

                if !launched {
                    // 5) Large-only: clickable dots, else background close
                    if large {
                        let mut dot_clicked = false;
                        for (rect, idx) in &dot_hits {
                            if point_in(mouse_pos, *rect) {
                                page = *idx;          // switch page, keep menu open
                                dot_clicked = true;
                                break;
                            }
                        }
                        if !dot_clicked {
                            // background click closes menu (unless on search bar)
                            if !point_in(mouse_pos, search_rect) {
                                break 'ev;
                            }
                        }
                    }
                    // Small mode: background click does nothing (intended)
                }
            }
        }

        last_mouse_down = mouse_down;
    }

    DesktopMenuResult::None
}
