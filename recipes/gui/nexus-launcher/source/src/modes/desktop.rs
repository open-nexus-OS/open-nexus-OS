use orbclient::{Color, EventOption, Renderer, Window, WindowFlag, K_ESC};
use orbfont::Font;
use orbimage::ResizeType;

use crate::ui::{compute_grid, draw_searchbar, filter_packages, grid_iter_and_hit};
use crate::config;
use crate::icons::CommonIcons;
use crate::themes::{BAR_HEIGHT, BAR_COLOR};

/// Local UI root for assets (avoid relying on private constants in main.rs)
#[cfg(target_os = "redox")]
const UI_PATH: &str = "/ui";
#[cfg(not(target_os = "redox"))]
const UI_PATH: &str = "ui";

/// Result of the desktop start menu
pub enum DesktopMenuResult {
    None,
    Launch(String),
    Logout, // request to switch to the login screen
}

/// Fill a rounded rectangle directly into the transparent window.
/// This draws horizontal spans per scanline so the window corners remain transparent.
/// `r` is the corner radius in pixels.
fn fill_round_rect(win: &mut Window, x: i32, y: i32, w: u32, h: u32, r: i32, color: Color) {
    let w_i = w as i32;
    let h_i = h as i32;

    // Fallback if radius is too large for the given size
    if r <= 0 || w < (2 * r as u32) || h < (2 * r as u32) {
        win.rect(x, y, w, h, color);
        return;
    }

    for yi in 0..h_i {
        // Determine how far we are inside the top/bottom rounded corners
        let dy = if yi < r {
            // top corner band
            r - 1 - yi
        } else if yi >= h_i - r {
            // bottom corner band
            yi - (h_i - r)
        } else {
            // middle band (no rounding)
            -1
        };

        let (start_x, end_x) = if dy >= 0 {
            // Inside the top/bottom corner band:
            // compute the x-offset by circle equation: x^2 + y^2 = r^2
            let dx = ((r * r - dy * dy) as f32).sqrt().floor() as i32;
            let sx = x + r - dx;
            let ex = x + w_i - r + dx;
            (sx, ex)
        } else {
            // Middle: full width
            (x, x + w_i)
        };

        let line_w = (end_x - start_x).max(0) as u32;
        if line_w > 0 {
            win.rect(start_x, y + yi, line_w, 1, color);
        }
    }
}

/// Returns a subtle hover fill color depending on mode:
/// - small (light panel): a faint dark veil
/// - large (dark overlay): a faint light veil
#[inline]
fn hover_fill_color(large: bool) -> orbclient::Color {
    if large {
        // brighten a bit on dark background
        Color::rgba(255, 255, 255, 28) // very subtle
    } else {
        // darken a bit on light background
        Color::rgba(0, 0, 0, 22)
    }
}

pub fn show_desktop_menu(screen_w: u32, screen_h: u32) -> DesktopMenuResult {
    // Collect packages once
    let mut pkgs = crate::get_packages();

    // Initial size variant (small/large) from config
    let mut large = config::desktop_large();

    // Create window with initial target geometry
    let (mut win_x, mut win_y, mut win_w, mut win_h) = target_geometry(screen_w, screen_h, large);
    let mut window = Window::new_flags(
        win_x, win_y, win_w, win_h, "StartMenuDesktop",
        &[
            WindowFlag::Async,
            WindowFlag::Borderless,
            WindowFlag::Transparent,
        ],
    ).expect("desktop menu window");

    let font = Font::find(Some("Sans"), None, None).unwrap();
    let mut query = String::new();
    let icons = CommonIcons::load(UI_PATH);

    // Username for the bottom-left badge
    let username = std::env::var("USER").unwrap_or_else(|_| "user".to_string());

    // Mouse state
    let mut mouse_pos = (0, 0);
    let mut mouse_down = false;
    let mut last_mouse_down = false;

    // Helper: top-right toggle icon rectangle
    let toggle_rect = |win: &Window, icon_w: i32, icon_h: i32| -> (i32, i32, i32, i32) {
        let rx = win.width() as i32 - icon_w - 10;
        let ry = 10;
        (rx, ry, icon_w, icon_h)
    };

    // Hitboxes for bottom-right controls
    let mut power_hit: (i32, i32, i32, i32);
    let mut settings_hit: (i32, i32, i32, i32);

    // Last frame's key UI rects
    let mut search_rect: (i32, i32, i32, i32);
    let mut toggle_hit: (i32, i32, i32, i32);

    'ev: loop {
        // Background by mode:
        //  - small: transparent window + draw a 5 px rounded white panel
        //  - large: dark overlay (covers everything except the taskbar)
        if large {
            window.set(Color::rgba(0, 0, 0, 80));
        } else {
            // Clear the window fully transparent first
            window.set(Color::rgba(0, 0, 0, 0));
            // Read size BEFORE taking &mut borrow
            let ww = window.width();
            let wh = window.height();
            // Draw the rounded panel (5 px radius)
            fill_round_rect(
                &mut window,
                0,
                0,
                ww,
                wh,
                5,
                Color::rgba(0xFF, 0xFF, 0xFF, 210),
            );
        }

        // Layout + search bar
        let layout = compute_grid(window.width(), window.height());
        let (sx, sy, sw, sh) = draw_searchbar(&mut window, &font, 14, &query);
        search_rect = (sx, sy, sw, sh);

        // Search icon
        let search_icon = if large { &icons.search_lg } else { &icons.search_sm };
        let sih = search_icon.height() as i32;
        search_icon.draw(&mut window, sx + 10, sy + (sh - sih) / 2);

        // Grid + click detection
        let filtered = filter_packages(&pkgs, &query);
        let clicked = grid_iter_and_hit(
            &mut window, &font, &layout, &mut pkgs, &filtered,
            Some((mouse_pos.0, mouse_pos.1, mouse_down, !mouse_down && last_mouse_down)),
            true,
        );

        // ===== Top-right Toggle (expand/shrink) =====
        let toggle_icon = if large { &icons.resize_lg } else { &icons.resize_sm };
        let tiw = toggle_icon.width() as i32;
        let tih = toggle_icon.height() as i32;
        let (rx, ry, rw, rh) = (window.width() as i32 - tiw - 10, 10, tiw, tih);
        toggle_hit = (rx, ry, rw, rh);

        // Hover background for toggle
        if point_in(mouse_pos, toggle_hit) {
            let pad = 6; // small padding around the icon
            fill_round_rect(&mut window, rx - pad, ry - pad, (rw + 2*pad) as u32, (rh + 2*pad) as u32, 6, hover_fill_color(large));
        }
        // Draw toggle icon on top
        toggle_icon.draw(&mut window, rx, ry);

        // ===== Bottom area =====
        let margin = 16i32;
        let gap = 16i32;

        // Avatar (scaled)
        let target_h = if large { 22 } else { 20 };
        let mut user_img = icons.user.clone();
        let target_h_u = target_h as u32;
        let target_w_u = ((user_img.width() * target_h_u) / user_img.height()).max(1);
        user_img = user_img.resize(target_w_u, target_h_u, orbimage::ResizeType::Lanczos3).unwrap();

        let user_x = margin;
        let user_y = window.height() as i32 - target_h - margin;

        // Username
        let base_name_color = if large {
            Color::rgba(0xFF, 0xFF, 0xFF, 230)
        } else {
            Color::rgba(0x14, 0x14, 0x14, 220) // slightly less than full; hover will bump to 255
        };
        let name_text = font.render(&username, 16.0);
        let name_x = user_x + user_img.width() as i32 + 8;
        let name_y = user_y + (target_h - name_text.height() as i32) / 2;

        // --- Compute user "hit" rect (avatar + text) ---
        let user_w = (name_x + name_text.width() as i32) - user_x;
        let user_h = target_h.max(name_text.height() as i32);
        let user_hit = (user_x, user_y.min(name_y), user_w, user_h);
        let user_hover = point_in(mouse_pos, user_hit);

        // Hover background for user
        if user_hover {
            let pad = 6;
            fill_round_rect(
                &mut window,
                user_hit.0 - pad,
                user_hit.1 - pad,
                (user_hit.2 + 2*pad) as u32,
                (user_hit.3 + 2*pad) as u32,
                6,
                hover_fill_color(large),
            );
        }

        // Draw avatar + text after hover background
        user_img.draw(&mut window, user_x, user_y);
        let name_col = if user_hover {
            // bump to full opacity on hover
            if large { Color::rgba(0xFF, 0xFF, 0xFF, 255) } else { Color::rgba(0x14, 0x14, 0x14, 255) }
        } else {
            base_name_color
        };
        name_text.draw(&mut window, name_x, name_y, name_col);

        // Right side controls (settings outer, power left)
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

        // Hover backgrounds for power/settings
        let pad = 6;
        if point_in(mouse_pos, settings_hit) {
            fill_round_rect(&mut window, settings_x - pad, settings_y - pad, (sw2 + 2*pad) as u32, (sh2 + 2*pad) as u32, 6, hover_fill_color(large));
        }
        if point_in(mouse_pos, power_hit) {
            fill_round_rect(&mut window, power_x - pad, power_y - pad, (pw + 2*pad) as u32, (ph + 2*pad) as u32, 6, hover_fill_color(large));
        }

        // Draw icons on top
        settings_icon.draw(&mut window, settings_x, settings_y);
        power_icon.draw(&mut window, power_x, power_y);

        // --- thin white separator above bottom controls (SMALL MODE ONLY) ---
        if !large {
            // Top edge of the bottom controls (icons' top y)
            let controls_top = user_y.min(settings_y).min(power_y);

            // Small gap above the icons (tweak if needed)
            const SEPARATOR_OFFSET: i32 = 8;
            let sep_y = (controls_top - SEPARATOR_OFFSET).max(0);

            // From the start of the user area to the RIGHT EDGE of the settings icon
            let sep_x1 = user_x;
            let sep_x2 = settings_x + sw2; // extend to settings

            if sep_x2 > sep_x1 {
                // 1-px line; adjust alpha for stronger/weaker look
                window.rect(sep_x1, sep_y, (sep_x2 - sep_x1) as u32, 1, Color::rgba(255, 255, 255, 230));
            }
        }

        window.sync();

        // Events
        for ev in window.events() {
            match ev.to_option() {
                EventOption::Key(k) if k.scancode == K_ESC && k.pressed => break 'ev,
                EventOption::Key(k) if k.pressed => {
                    // In your orbclient, `character` is `char`; '\0' means non-printable.
                    let ch = k.character;
                    match ch {
                        '\u{8}' => { query.pop(); } // Backspace
                        '\n' | '\r' => {}
                        c if c != '\0' && !c.is_control() => query.push(c),
                        _ => {}
                    }
                }
                EventOption::Mouse(m) => { mouse_pos = (m.x, m.y); }
                EventOption::Button(b) => { mouse_down = b.left; }
                // Close when this window loses focus (clicking desktop, taskbar, or Start again)
                EventOption::Focus(f) => {
                    if !f.focused {
                        break 'ev;
                    }
                }
                EventOption::Quit(_) => break 'ev,
                _ => {}
            }
        }

        // Click release edge
        if !mouse_down && last_mouse_down {
            // Toggle: update state and resize THIS window in-place (no new window)
            if point_in(mouse_pos, toggle_hit) {
                large = !large;
                config::set_desktop_large(large);
                (win_x, win_y, win_w, win_h) = target_geometry(screen_w, screen_h, large);
                window.set_pos(win_x, win_y);
                window.set_size(win_w, win_h);
            } else if point_in(mouse_pos, settings_hit) {
                // Settings: close menu (MVP behavior)
                break 'ev;
            } else if point_in(mouse_pos, power_hit) {
                // Power: request logout
                return DesktopMenuResult::Logout;
            } else if large {
                // In LARGE mode: clicking on the dark background (outside UI) closes the menu.
                if !point_in(mouse_pos, search_rect) {
                    break 'ev;
                }
            }
        }

        // Launch app if grid item was clicked
        if let Some(exec) = clicked {
            return DesktopMenuResult::Launch(exec);
        }

        last_mouse_down = mouse_down;
    }

    DesktopMenuResult::None
}

/// Compute target desktop menu geometry for small/large modes.
///
/// Small:
///   - horizontally centered
///   - vertically positioned 11 px above the taskbar:
///       y = screen_h - BAR_HEIGHT - 11 - h
///
/// Large:
///   - covers the whole screen EXCEPT the taskbar height
///   - leaves `BAR_HEIGHT` pixels at the bottom so the bar stays accessible
fn target_geometry(screen_w: u32, screen_h: u32, large: bool) -> (i32, i32, u32, u32) {
    if large {
        let x = 0;
        let y = 0;
        let w = screen_w;
        let h = screen_h.saturating_sub(BAR_HEIGHT);
        (x, y, w, h)
    } else {
        let w = (screen_w as f32 * 0.46) as u32;
        let h = (screen_h as f32 * 0.42) as u32;
        let x = ((screen_w - w) / 2) as i32;
        // 11 px above the taskbar:
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
