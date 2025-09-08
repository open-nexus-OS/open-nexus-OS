use orbclient::{Color, EventOption, Renderer, Window, WindowFlag, K_ESC};
use orbfont::Font;
use orbimage::ResizeType;

use crate::ui::{compute_grid, draw_searchbar, filter_packages, grid_iter_and_hit};
use crate::icons::CommonIcons;

/// Local UI root for assets
#[cfg(target_os = "redox")]
const UI_PATH: &str = "/ui";
#[cfg(not(target_os = "redox"))]
const UI_PATH: &str = "ui";

pub enum MobileMenuResult {
    None,
    Launch(String),
    Logout, // request to switch to the login screen
}

pub fn show_mobile_menu(screen_w: u32, screen_h: u32) -> MobileMenuResult {
    let mut pkgs = crate::get_packages();

    // Fullscreen overlay
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

    // Hitboxes we track
    let mut power_hit = (0, 0, 0, 0);
    let mut settings_hit = (0, 0, 0, 0);
    let mut search_rect = (0, 0, 0, 0); // for background-close exception

    'ev: loop {
        // Dark overlay
        window.set(Color::rgba(0, 0, 0, 80));

        // Layout + searchbar
        let layout = compute_grid(window.width(), window.height());
        let (sx, sy, sw, sh) = draw_searchbar(&mut window, &font, layout.top - 36, &query);
        search_rect = (sx, sy, sw, sh);

        // White search icon
        let search_icon = &icons.search_lg;
        let sih = search_icon.height() as i32;
        search_icon.draw(&mut window, sx + 10, sy + (sh - sih) / 2);

        // Grid + click detection
        let filtered = filter_packages(&pkgs, &query);
        let clicked = grid_iter_and_hit(
            &mut window, &font, &layout, &mut pkgs, &filtered,
            Some((mouse_pos.0, mouse_pos.1, mouse_down, !mouse_down && last_mouse_down)),
            true,
        );

        // ===== Bottom area =====
        let margin = 20;
        let gap = 24;

        // Left: small user avatar + name
        let target_h = 22u32;
        let mut user_img = icons.user.clone().resize(
            ((icons.user.width() * target_h) / icons.user.height()).max(1),
            target_h,
            ResizeType::Lanczos3
        ).unwrap();

        let user_x = margin;
        let user_y = window.height() as i32 - user_img.height() as i32 - margin;
        user_img.draw(&mut window, user_x, user_y);

        let name_text = font.render(&username, 16.0);
        let name_x = user_x + user_img.width() as i32 + 8;
        let name_y = user_y + (user_img.height() as i32 - name_text.height() as i32) / 2;
        name_text.draw(&mut window, name_x, name_y, Color::rgba(0xFF, 0xFF, 0xFF, 230));

        // Right: settings (outermost right) + power next to it (left)
        let settings_icon = &icons.settings_lg;
        let power_icon = &icons.power_lg;

        let sw2 = settings_icon.width() as i32;
        let sh2 = settings_icon.height() as i32;
        let pw = power_icon.width() as i32;
        let ph = power_icon.height() as i32;

        let settings_x = window.width() as i32 - sw2 - margin;
        let settings_y = window.height() as i32 - sh2 - margin;

        let power_x = settings_x - gap - pw;
        let power_y = window.height() as i32 - ph - margin;

        settings_icon.draw(&mut window, settings_x, settings_y);
        power_icon.draw(&mut window, power_x, power_y);

        settings_hit = (settings_x, settings_y, sw2, sh2);
        power_hit    = (power_x,    power_y,    pw, ph);

        window.sync();

        // Events
        for ev in window.events() {
            match ev.to_option() {
                EventOption::Key(k) if k.scancode == K_ESC && k.pressed => break 'ev,
                EventOption::Key(k) if k.pressed => {
                    let ch = k.character; // `char` in this orbclient
                    match ch {
                        '\u{8}' => { query.pop(); } // Backspace
                        '\n' | '\r' => {}
                        c if c != '\0' && !c.is_control() => query.push(c),
                        _ => {}
                    }
                }
                EventOption::Mouse(m) => { mouse_pos = (m.x, m.y); }
                EventOption::Button(b) => { mouse_down = b.left; }
                // NEW: close on focus loss (clicking desktop/taskbar/Start button)
                EventOption::Focus(f) => {
                    if !f.focused {
                        break 'ev;
                    }
                }
                EventOption::Quit(_) => break 'ev,
                _ => {}
            }
        }

        // Bottom controls: close (settings) or logout (power)
        if !mouse_down && last_mouse_down {
            if point_in(mouse_pos, settings_hit) {
                break 'ev;
            }
            if point_in(mouse_pos, power_hit) {
                return MobileMenuResult::Logout;
            }
            // In fullscreen mobile: click on dark background closes the menu
            if !point_in(mouse_pos, search_rect) {
                // If grid cell was clicked, we would already have returned a Launch
                break 'ev;
            }
        }

        if let Some(exec) = clicked {
            return MobileMenuResult::Launch(exec);
        }

        last_mouse_down = mouse_down;
    }

    MobileMenuResult::None
}

#[inline]
fn point_in(p: (i32, i32), r: (i32, i32, i32, i32)) -> bool {
    let (x, y) = p;
    let (rx, ry, rw, rh) = r;
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}
