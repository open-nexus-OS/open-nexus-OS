// src/mobile.rs
// Mobile start menu (fullscreen) using the shared ui::draw_app_cell.

use orbclient::{Color, EventOption, Renderer, Window, WindowFlag, K_ESC, K_LEFT, K_RIGHT};
use orbfont::Font;
use orbimage::ResizeType;

use crate::icons::CommonIcons;
use crate::ui;

#[cfg(target_os = "redox")]
const UI_PATH: &str = "/ui";
#[cfg(not(target_os = "redox"))]
const UI_PATH: &str = "ui";

pub enum MobileMenuResult {
    None,
    Launch(String),
    Logout,
}

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

#[inline]
fn point_in(p: (i32, i32), r: (i32, i32, i32, i32)) -> bool {
    let (x, y) = p;
    let (rx, ry, rw, rh) = r;
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

pub fn show_mobile_menu(screen_w: u32, screen_h: u32, pkgs: &mut [crate::package::Package]) -> MobileMenuResult {
    let mut window = Window::new_flags(
        0, 0, screen_w, screen_h, "StartMenuMobile",
        &[WindowFlag::Async, WindowFlag::Borderless, WindowFlag::Transparent],
    ).expect("mobile menu window");

    let font = Font::find(Some("Sans"), None, None).unwrap();
    let icons = CommonIcons::load(UI_PATH);
    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());

    let mut query = String::new();
    let mut mouse_pos = (0, 0);
    let mut mouse_down = false;
    let mut last_mouse_down = false;

    // Paging state
    let mut page: usize = 0;

    // Hitboxes
    let mut power_hit: (i32,i32,i32,i32);
    let mut settings_hit: (i32,i32,i32,i32);
    let mut search_rect: (i32,i32,i32,i32);

    'ev: loop {
        // Dark overlay
        window.set(Color::rgba(0, 0, 0, 80));

        // Search bar
        let pad = 16i32;
        let sx = pad;
        let sy = pad;
        let sw = window.width() as i32 - pad*2;
        let sh = 36i32;

        fill_round_rect(&mut window, sx, sy, sw as u32, sh as u32, 8, Color::rgba(255,255,255,26));
        let qtxt = if query.is_empty() { "Search apps…" } else { &query };
        let qcol = if query.is_empty() { Color::rgba(255,255,255,150) } else { Color::rgba(255,255,255,230) };
        let dpi = crate::dpi_scale();
        let q = font.render(qtxt, 14.0 * dpi);
        q.draw(&mut window, sx + 10, sy + (sh - q.height() as i32)/2, qcol);

        search_rect = (sx, sy, sw, sh);

        // Filter
        let indices: Vec<usize> = if query.trim().is_empty() {
            (0..pkgs.len()).collect()
        } else {
            let ql = query.to_lowercase();
            pkgs.iter().enumerate().filter_map(|(i,p)| {
                if p.name.to_lowercase().contains(&ql) { Some(i) } else { None }
            }).collect()
        };

        // Grid layout
        let content_x = pad;
        let content_y = sy + sh + 16;
        let content_w = window.width() as i32 - pad*2;

        let bottom_reserve = 72i32; // room for dots & bottom controls
        let content_h = (window.height() as i32 - bottom_reserve) - content_y;

        let landscape = window.width() > window.height();
        let cols = if landscape { 8usize } else { 5usize };

        let hgap = 16;
        let vgap = 16;
        let cell_w = ((content_w - (cols as i32 - 1) * hgap) / cols as i32).max(54);
        let target_rows = 5usize;
        let cell_h_guess = (content_h - ((target_rows as i32 - 1) * vgap)) / target_rows as i32;
        let cell_h = cell_h_guess.max(72);
        let rows = ((content_h + vgap) / (cell_h + vgap)).max(3) as usize;

        let per_page = cols * rows;
        let page_count = if per_page == 0 { 1 } else { ((indices.len() + per_page - 1) / per_page).max(1) };
        if page >= page_count { page = page_count - 1; }

        // Visible slice
        let start = page * per_page;
        let end = (start + per_page).min(indices.len());
        let slice = indices.get(start..end).unwrap_or(&[]);

        // Draw cells
        let icon_side = {
            let dpi = crate::dpi_scale();
            (cell_w as f32 * 0.82 * dpi).round().clamp(32.0, 96.0) as i32
        };
        let mut cells: Vec<((i32,i32,i32,i32), usize)> = Vec::new();
        for (i, idx) in slice.iter().enumerate() {
            let row = (i / cols) as i32;
            let col = (i % cols) as i32;
            let cx = content_x + col * (cell_w + hgap);
            let cy = content_y + row * (cell_h + vgap);

            if point_in(mouse_pos, (cx, cy, cell_w, cell_h)) {
                fill_round_rect(&mut window, cx, cy, cell_w as u32, cell_h as u32, 10, Color::rgba(255,255,255,26));
            }

            let rect = ui::draw_app_cell(
                &mut window, &font, &mut pkgs[*idx],
                cx, cy, cell_w, cell_h,
                icon_side, true, true, // large=true for fullscreen
            );
            cells.push((rect, *idx));
        }

        // Bottom controls (right): settings & power
        let margin = 20i32;
        let gap = 24i32;

        let (settings_icon, power_icon) = (&icons.settings_lg, &icons.power_lg);
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

        settings_icon.draw(&mut window, settings_x, settings_y);
        power_icon.draw(&mut window, power_x, power_y);

        // User (left bottom)
        let target_h = 22u32;
        let user_img = icons.user.clone().resize(
            ((icons.user.width() * target_h) / icons.user.height()).max(1),
            target_h,
            ResizeType::Lanczos3
        ).unwrap();

        let user_x = margin;
        let user_y = window.height() as i32 - user_img.height() as i32 - margin;
        user_img.draw(&mut window, user_x, user_y);

        let dpi = crate::dpi_scale();
        let name_text = font.render(&username, 16.0 * dpi);
        let name_x = user_x + user_img.width() as i32 + 8;
        let name_y = user_y + (user_img.height() as i32 - name_text.height() as i32) / 2;
        name_text.draw(&mut window, name_x, name_y, Color::rgba(0xFF, 0xFF, 0xFF, 230));

        // Page dots
        if page_count > 1 {
            let dots_y = (window.height() as i32 - sh2 - margin) - 18;
            let dot_w = 8;
            let dot_h = 3;
            let spacing = 8;
            let total_w = (page_count as i32) * dot_w + ((page_count as i32 - 1) * spacing);
            let mut dx = (window.width() as i32 - total_w) / 2;
            for i in 0..page_count {
                let active = i == page;
                let a = if active { 220 } else { 90 };
                window.rect(dx, dots_y, dot_w as u32, dot_h as u32, Color::rgba(255,255,255,a));
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
                    match ch {
                        '\u{8}' => { query.pop(); }
                        '\n' | '\r' => {}
                        c if c != '\0' && !c.is_control() => query.push(c),
                        _ => {}
                    }
                    match k.scancode {
                        K_LEFT  => { if page > 0 { page -= 1; } }
                        K_RIGHT => { if page + 1 < page_count { page += 1; } }
                        _ => {}
                    }
                }
                EventOption::Mouse(m) => { mouse_pos = (m.x, m.y); }
                EventOption::Button(b) => { mouse_down = b.left; }
                EventOption::Focus(f) => { if !f.focused { break 'ev; } }
                EventOption::Quit(_) => break 'ev,
                _ => {}
            }
        }

        // Release edge
        if !mouse_down && last_mouse_down {
            if point_in(mouse_pos, settings_hit) { break 'ev; }
            else if point_in(mouse_pos, power_hit) { return MobileMenuResult::Logout; }
            else if !point_in(mouse_pos, search_rect) {
                for (rect, idx) in &cells {
                    if point_in(mouse_pos, *rect) {
                        let exec = pkgs[*idx].exec.clone();
                        if !exec.trim().is_empty() {
                            return MobileMenuResult::Launch(exec);
                        }
                    }
                }
                // background click closes menu
                break 'ev;
            }
        }

        last_mouse_down = mouse_down;
    }

    MobileMenuResult::None
}
