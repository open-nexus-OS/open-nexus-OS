#![forbid(clippy::unwrap_used)]
#![forbid(clippy::expect_used)]

extern crate log;
extern crate orbclient;
extern crate orbfont;
extern crate orbimage;
extern crate redox_log;
extern crate redox_users;

use log::{error};
use std::process::Command;
use std::{env, io, str};
use std::time::{Duration, Instant};

use orbclient::{Color, EventOption, Renderer, Window, WindowFlag};
use orbfont::Font;
use orbimage::Image;
use redox_log::{OutputBuilder, RedoxLogger};
use redox_users::{All, AllUsers, Config};
use libredox::flag;

// -------- UI THEME --------
const PANEL_BG: Color = Color::rgba(255, 255, 255, 191);   // 75% white
const LABEL:    Color = Color::rgb(0xEE, 0xEE, 0xEE);
const LABEL_D:  Color = Color::rgb(0xCC, 0xCC, 0xCC);
const ACCENT:   Color = Color::rgb(0x7A, 0xB7, 0xFF);
const ERROR:    Color = Color::rgb(0xFF, 0x55, 0x55);

const AVATAR_RADIUS: i32 = 64;      // avatar circle radius
const PANEL_PAD: i32 = 16;          // panel inner padding
const FIELD_H: i32 = 36;            // password field height
const BTN_H: i32 = 36;              // bottom action buttons height
const BTN_GAP: i32 = 28;            // spacing between bottom buttons

// --- Actions bar layout constants (keep in sync across call sites) ---
const ACTIONS_SLOT_W: i32         = 120; // width per button
const ACTIONS_SLOT_H: i32         = 92;  // total height (icon + text + padding)
const ACTIONS_BOTTOM_PADDING: i32 = 40;  // distance of the whole bar from the bottom
const ICON_BTN_SIZE: i32          = 36;  // target icon size inside each button
const ICON_TEXT_GAP: i32          = 6;   // vertical gap between icon and text

// Actions
#[derive(Clone, Copy, Debug)]
enum Action { Sleep, Restart, Shutdown, Logout }

// Login state machine
#[derive(Clone, Debug)]
enum AppState {
    SelectUser { users: Vec<String>, hover: Option<usize> },
    EnterPassword { user: String, password: String, focus_pwd: bool, show_error: bool },
}

#[derive(Clone, Copy)]
enum BackgroundMode {
    /// Do not resize the image, just center it
    Center,
    /// Resize the image to the display size
    Fill,
    /// Resize the image - keeping its aspect ratio, and fit it to the display with blank space
    Scale,
    /// Resize the image - keeping its aspect ratio, and crop to remove all blank space
    Zoom,
}

impl BackgroundMode {
    fn from_str(string: &str) -> BackgroundMode {
        match string {
            "fill" => BackgroundMode::Fill,
            "scale" => BackgroundMode::Scale,
            "zoom" => BackgroundMode::Zoom,
            _ => BackgroundMode::Center,
        }
    }
}

// Simple rectangle for hit testing
#[derive(Clone, Copy, Debug)]
struct Rect { pub x: i32, pub y: i32, pub w: u32, pub h: u32 }
impl Rect {
    fn new(x: i32, y: i32, w: u32, h: u32) -> Self { Self { x, y, w, h } }
    fn contains(&self, mx: i32, my: i32) -> bool {
        mx >= self.x && mx < self.x + self.w as i32 &&
        my >= self.y && my < self.y + self.h as i32
    }
}

struct ActionIcons {
    sleep:   Option<Image>,
    restart: Option<Image>,
    shutdown: Option<Image>,
    logout:  Option<Image>,
}

// CACHE for background image scaling
struct CachedBackground {
    original: Image,
    scaled: Option<Image>,
    last_size: (u32, u32),
}

impl CachedBackground {
    fn new(image: Image) -> Self {
        Self {
            original: image,
            scaled: None,
            last_size: (0, 0),
        }
    }
    
    fn get_scaled(&mut self, width: u32, height: u32) -> &Image {
        if self.last_size != (width, height) || self.scaled.is_none() {
            let scaled = self.original.resize(width, height, orbimage::ResizeType::Lanczos3).unwrap();
            self.scaled = Some(scaled);
            self.last_size = (width, height);
        }
        self.scaled.as_ref().unwrap()
    }
}

fn find_scale(
    image: &Image,
    mode: BackgroundMode,
    display_width: u32,
    display_height: u32,
) -> (u32, u32) {
    match mode {
        BackgroundMode::Center => (image.width(), image.height()),
        BackgroundMode::Fill => (display_width, display_height),
        BackgroundMode::Scale => {
            let d_w = display_width as f64;
            let d_h = display_height as f64;
            let i_w = image.width() as f64;
            let i_h = image.height() as f64;

            let scale = if d_w / d_h > i_w / i_h {
                d_h / i_h
            } else {
                d_w / i_w
            };

            ((i_w * scale) as u32, (i_h * scale) as u32)
        }
        BackgroundMode::Zoom => {
            let d_w = display_width as f64;
            let d_h = display_height as f64;
            let i_w = image.width() as f64;
            let i_h = image.height() as f64;

            let scale = if d_w / d_h < i_w / i_h {
                d_h / i_h
            } else {
                d_w / i_w
            };

            ((i_w * scale) as u32, (i_h * scale) as u32)
        }
    }
}

fn normal_usernames() -> Vec<String> {
    let Ok(users) = AllUsers::authenticator(Config::default()) else {
        return vec!["nexus".into()];
    };
    let mut names: Vec<String> = users.iter()
        .filter(|u| u.uid >= 1000) // non-root
        .map(|u| u.user.clone())
        .collect();
    names.sort();
    if names.is_empty() { names.push("nexus".into()); }
    names
}

fn create_fallback_image(width: u32, height: u32) -> Image {
    // Build a solid-colored image via from_data (no per-pixel setter needed)
    let px = Color::rgb(40, 40, 120);
    let mut data = Vec::with_capacity((width * height) as usize);
    data.resize((width * height) as usize, px);
    // Safe unwrap: width*height >= 1 here; if width/height could be 0, guard before call
    Image::from_data(width, height, data.into()).unwrap()
}

fn login_command(
    username: &str,
    pass: &str,
    launcher_cmd: &str,
    launcher_args: &[String],
) -> Option<Command> {
    let sys_users = match AllUsers::authenticator(Config::default()) {
        Ok(users) => users,
        // Not maybe the best thing to do...
        Err(_) => return None,
    };

    match sys_users.get_by_name(&username) {
        Some(user) => {
            if user.verify_passwd(&pass) {
                let mut command = user.login_cmd(&launcher_cmd);
                for arg in launcher_args.iter() {
                    command.arg(&arg);
                }

                Some(command)
            } else {
                None
            }
        }
        None => None,
    }
}

fn login_window(launcher_cmd: &str, launcher_args: &[String]) -> Result<Option<Command>, String> {
    // Font and display metrics
    let font = Font::find(Some("Sans"), None, None)?;
    let (display_width, display_height) = orbclient::get_display_size()?;

    // Fullscreen, borderless window
    let mut window = Window::new_flags(
        0, 0, display_width, display_height, "nexus_login",
        &[
            WindowFlag::Borderless,
            WindowFlag::Unclosable,
            WindowFlag::Back, // draw over previous content (no transparency)
        ],
    ).ok_or("Could not create window")?;

    // --- Background image with resize cache ---
    let image_path = "/ui/login.png";
    let original_image = match Image::from_path(image_path) {
        Ok(img) => {
            log::info!("Loaded background image {}: {}x{}", image_path, img.width(), img.height());
            img
        }
        Err(e) => {
            error!("Login background not found at {}: {}", image_path, e);
            create_fallback_image(display_width.max(1), display_height.max(1))
        }
    };
    let mut bg_cache = CachedBackground::new(original_image);

    // Action icons (optional assets)
    let action_icons = ActionIcons {
        sleep:    Image::from_path("/ui/icons/actions/system-sleep.png").ok(),
        restart:  Image::from_path("/ui/icons/actions/system-restart.png").ok(),
        shutdown: Image::from_path("/ui/icons/actions/system-shut-down.png").ok(),
        logout:   Image::from_path("/ui/icons/actions/system-log-out.png").ok(),
    };

    // Initial UI state
    let mut state = {
        let mut users = normal_usernames();
        if users.is_empty() { users.push("nexus".into()); }
        AppState::SelectUser { users, hover: None }
    };

    // Redraw scheduling
    let mut last_clock_redraw = Instant::now();
    let mut resize = Some((display_width, display_height));
    let mut dirty = true;
    const CLOCK_INTERVAL: Duration = Duration::from_secs(1);

    // Track last mouse position for hover invalidation
    let mut mouse_x = 0;
    let mut mouse_y = 0;
    let mut last_mouse_pos = (0, 0);

    loop {
        // Decide if a redraw is needed (dirty, resize or clock tick)
        let redraw_needed = dirty || resize.is_some() || last_clock_redraw.elapsed() >= CLOCK_INTERVAL;

        if redraw_needed {
            // 1) Draw cached background scaled to the current window size
            let bg_image = bg_cache.get_scaled(window.width(), window.height());
            bg_image.draw(&mut window, 0, 0);

            // 2) Draw UI content for current state
            let y_actions = window.height() as i32 - ACTIONS_SLOT_H - ACTIONS_BOTTOM_PADDING;

            match &state {
                AppState::SelectUser { users, hover } => {
                    // Select screen draws avatar (85% / 100% handled inside), username, and list
                    let _ = draw_select_state(&font, &mut window, users, *hover, Some((mouse_x, mouse_y)));
                }
                AppState::EnterPassword { user, password, focus_pwd, show_error } => {
                    // Password screen draws avatar (always 100%), username and field
                    let _ = draw_password_state(&font, &mut window, user, password, *focus_pwd, *show_error);
                }
            }

            // 3) Draw actions bar once (both states)
            let is_password_state = matches!(state, AppState::EnterPassword { .. });
            let _ = draw_actions_bar(&font, &mut window, y_actions, is_password_state, &action_icons, Some((mouse_x, mouse_y)));

            // Present
            window.sync();

            // Bookkeeping
            last_clock_redraw = Instant::now();
            dirty = false;
            resize = None;
        }

        // Non-blocking event pump
        for event in window.events() {
            match event.to_option() {
                EventOption::Mouse(m) => {
                    // Update mouse and invalidate on real motion (for hover effects)
                    last_mouse_pos = (mouse_x, mouse_y);
                    mouse_x = m.x;
                    mouse_y = m.y;
                    if (mouse_x, mouse_y) != last_mouse_pos {
                        dirty = true;
                    }

                    // Update hover in SelectUser state
                    if let AppState::SelectUser { users, hover } = &mut state {
                        let w = window.width() as i32;
                        let h = window.height() as i32;
                        let center_x = w / 2;
                        let center_y = h / 2 - 40;

                        // Avatar/name hitboxes
                        let avatar_rect = Rect::new(
                            center_x - AVATAR_RADIUS, center_y - AVATAR_RADIUS,
                            (AVATAR_RADIUS * 2) as u32, (AVATAR_RADIUS * 2) as u32
                        );
                        let name_rect = Rect::new(center_x - 75, center_y + AVATAR_RADIUS + 8, 150, 24);

                        let mut new_hover = None;
                        if avatar_rect.contains(mouse_x, mouse_y) || name_rect.contains(mouse_x, mouse_y) {
                            new_hover = Some(hover.unwrap_or(0).min(users.len().saturating_sub(1)));
                        } else if users.len() > 1 {
                            // Username row (simple centered layout)
                            let slot_w = 140;
                            let gap = 16;
                            let total = users.len() as i32 * slot_w + (users.len() as i32 - 1) * gap;
                            let mut x = center_x - total / 2;
                            let y = center_y + AVATAR_RADIUS + 32 + 8;
                            for (i, _) in users.iter().enumerate() {
                                let rect = Rect::new(x, y, slot_w as u32, 28);
                                if rect.contains(mouse_x, mouse_y) {
                                    new_hover = Some(i);
                                    break;
                                }
                                x += slot_w + gap;
                            }
                        } else {
                            // new hover = Some(0); No hover when there is only one user
                            new_hover = None; // only hover when the pointer is really over the avatar/name
                        }

                        if new_hover != *hover {
                            *hover = new_hover;
                            dirty = true;
                        }
                    }
                }

                EventOption::Button(b) => {
                    if !b.left { continue; }
                    match &mut state {
                        AppState::SelectUser { users, hover } => {
                            // Clicking on selected user advances to password
                            if let Some(sel) = *hover {
                                let user = users[sel].clone();
                                state = AppState::EnterPassword {
                                    user,
                                    password: String::new(),
                                    focus_pwd: true,
                                    show_error: false,
                                };
                                dirty = true;
                                continue;
                            }
                            // Bottom action buttons
                            let y_actions = window.height() as i32 - ACTIONS_SLOT_H - ACTIONS_BOTTOM_PADDING;
                            let actions = get_actions_hitboxes(&mut window, y_actions, false, &action_icons);
                            for (act, rect) in actions {
                                if rect.contains(mouse_x, mouse_y) {
                                    handle_action(act);
                                    dirty = true;
                                }
                            }
                        }

                        AppState::EnterPassword { user, password, focus_pwd, show_error } => {
                            // Back and field hitboxes
                            let (back_rect, field_rect) = get_password_hitboxes(&mut window, user, password, *focus_pwd, *show_error);

                            if back_rect.contains(mouse_x, mouse_y) {
                                state = AppState::SelectUser { users: normal_usernames(), hover: None };
                                dirty = true;
                                continue;
                            }

                            *focus_pwd = field_rect.contains(mouse_x, mouse_y);

                            // Bottom action buttons
                            let y_actions = window.height() as i32 - ACTIONS_SLOT_H - ACTIONS_BOTTOM_PADDING;
                            let actions = get_actions_hitboxes(&mut window, y_actions, true, &action_icons);
                            for (act, rect) in actions {
                                if rect.contains(mouse_x, mouse_y) {
                                    if let Action::Logout = act {
                                        state = AppState::SelectUser { users: normal_usernames(), hover: None };
                                    } else {
                                        handle_action(act);
                                    }
                                    dirty = true;
                                }
                            }
                        }
                    }
                }

                EventOption::Key(k) if k.pressed => {
                    match &mut state {
                        AppState::SelectUser { users, hover } => {
                            match k.scancode {
                                orbclient::K_LEFT  => { if let Some(hh) = hover { if *hh > 0 { *hh -= 1; } } else if !users.is_empty() { *hover = Some(0); } dirty = true; }
                                orbclient::K_RIGHT => { if let Some(hh) = hover { if *hh + 1 < users.len() { *hh += 1; } } else if users.len() > 1 { *hover = Some(1); } dirty = true; }
                                orbclient::K_ENTER => {
                                    let i = hover.unwrap_or(0).min(users.len().saturating_sub(1));
                                    let user = users[i].clone();
                                    state = AppState::EnterPassword { user, password: String::new(), focus_pwd: true, show_error: false };
                                    dirty = true;
                                }
                                _ => {}
                            }
                        }
                        AppState::EnterPassword { user, password, focus_pwd, show_error } => {
                            match k.scancode {
                                orbclient::K_ESC => {
                                    state = AppState::SelectUser { users: normal_usernames(), hover: None };
                                    dirty = true;
                                }
                                orbclient::K_BKSP => {
                                    if *focus_pwd { password.pop(); dirty = true; }
                                }
                                orbclient::K_ENTER => {
                                    if let Some(cmd) = login_command(user, password, launcher_cmd, launcher_args) {
                                        return Ok(Some(cmd));
                                    } else {
                                        *show_error = true;
                                        dirty = true;
                                    }
                                }
                                _ => {
                                    if *focus_pwd && k.character != '\0' {
                                        password.push(k.character);
                                        dirty = true;
                                    }
                                }
                            }
                        }
                    }
                }

                EventOption::Resize(r) => {
                    // Update window size and schedule background rescale
                    window.set_size(r.width, r.height);
                    resize = Some((r.width, r.height));
                    dirty = true;
                }
                EventOption::Screen(s) => {
                    // Handle screen geometry changes (e.g., mode switch)
                    window.set_size(s.width, s.height);
                    resize = Some((s.width, s.height));
                    dirty = true;
                }
                EventOption::Quit(_) => return Ok(None),
                _ => {}
            }
        }

        // Small sleep to reduce CPU usage when idle
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn handle_action(action: Action) {
    // TODO: Map these to your Redox commands or services.
    // I keep them as no-ops for now.
    let cmd = match action {
        Action::Sleep    => Some(("powerctl", &["suspend"][..])),
        Action::Restart  => Some(("reboot",   &[][..])),
        Action::Shutdown => Some(("poweroff", &[][..])),
        Action::Logout   => None, // handled in code by returning to SelectUser
    };
    if let Some((bin, args)) = cmd {
        let _ = Command::new(bin).args(args).spawn();
    }
}

// Draw an image fullscreen with 'Zoom' fit (cover), preserve aspect ratio
fn draw_fullscreen_zoom(win: &mut Window, img: &Image) {
    let w = win.width();
    let h = win.height();
    // Zielgröße berechnen
    let iw = img.width();
    let ih = img.height();
    if iw == 0 || ih == 0 { return; }
    let s = (w as f32 / iw as f32).max(h as f32 / ih as f32);
    let dw = (iw as f32 * s).round() as u32;
    let dh = (ih as f32 * s).round() as u32;

    let scaled = if dw == iw && dh == ih {
        img.clone()
    } else {
        img.resize(dw, dh, orbimage::ResizeType::Lanczos3).unwrap_or_else(|_| img.clone())
    };

    // Crop mittig
    let crop_x = (dw.saturating_sub(w)) / 2;
    let crop_y = (dh.saturating_sub(h)) / 2;
    let roi = scaled.roi(crop_x, crop_y, w, h);

    // Vollbild zeichnen
    roi.draw(win, 0, 0);
}

// Draw a clock in the top-right corner
fn draw_top_right_clock(font: &Font, win: &mut Window) {
    // Draw a simple HH:MM clock in the top-right corner
    if let Ok((w, _h)) = orbclient::get_display_size() {
        if let Ok(ts) = libredox::call::clock_gettime(flag::CLOCK_REALTIME) {
            let s = ts.tv_sec;
            let h = (s % 86400) / 3600;
            let m = (s / 60) % 60;

            let text = format!("{:02}:{:02}", h, m);
            let layout = font.render(&text, 18.0);
            let x = w as i32 - layout.width() as i32 - 16;
            layout.draw(win, x, 12, LABEL_D);
        }
    }
}

// Draw an image centered into rect, preserving aspect ratio and avoiding upscale
fn draw_image_centered(win: &mut Window, img: &orbimage::Image, rect: Rect) {
    let iw = img.width() as i32;
    let ih = img.height() as i32;
    let tw = rect.w as i32;
    let th = rect.h as i32;

    let (dw, dh) = if iw <= tw && ih <= th {
        (iw, ih) // no upscale
    } else {
        let sx = tw as f32 / iw as f32;
        let sy = th as f32 / ih as f32;
        let s = sx.min(sy);
        ((iw as f32 * s).round() as i32, (ih as f32 * s).round() as i32)
    };

    let x = rect.x + (tw - dw) / 2;
    let y = rect.y + (th - dh) / 2;
    if dw == iw && dh == ih {
        img.draw(win, x, y);
    } else {
        img.resize(dw as u32, dh as u32, orbimage::ResizeType::Lanczos3)
            .unwrap()
            .draw(win, x, y);
    }
}

// Draws a circular user avatar placeholder at the given center position.
fn draw_user_avatar(win: &mut Window, center_x: i32, center_y: i32) {
    draw_user_avatar_with_opacity(win, center_x, center_y, 255);
}

/// Draws the bottom actions (Sleep/Restart/Shutdown[/Logout]) as icon-buttons.
fn draw_actions_bar(
    font: &Font,
    win: &mut Window,
    y: i32,
    state_is_pwd: bool,
    icons: &ActionIcons,
    mouse: Option<(i32, i32)>,
) -> Vec<(Action, Rect)> {
    // Assemble items and their icons (some icons may be None)
    let mut items: Vec<(Action, &str, Option<&Image>)> = vec![
        (Action::Sleep,    "Sleep",    icons.sleep.as_ref()),
        (Action::Restart,  "Restart",  icons.restart.as_ref()),
        (Action::Shutdown, "Shutdown", icons.shutdown.as_ref()),
    ];
    if state_is_pwd {
        items.push((Action::Logout, "Logout", icons.logout.as_ref()));
    }

    let w      = win.width() as i32;
    let n      = items.len() as i32;
    let total_w = n * ACTIONS_SLOT_W + (n - 1) * BTN_GAP;
    let mut x   = (w - total_w) / 2;

    let mut hits: Vec<(Action, Rect)> = Vec::new();

    for (act, label, icon_opt) in items {
        // Full slot hitbox
        let rect = Rect::new(x, y, ACTIONS_SLOT_W as u32, ACTIONS_SLOT_H as u32);

        // Hover highlight (subtle)
        if let Some((mx, my)) = mouse {
            if rect.contains(mx, my) {
                win.rect(rect.x, rect.y, rect.w, rect.h, Color::rgba(255, 255, 255, 32));
            }
        }

        // Draw icon centered horizontally, higher in the slot
        if let Some(icon) = icon_opt {
            // Scale icon to ICON_BTN_SIZE, preserving aspect ratio
            let iw = icon.width()  as i32;
            let ih = icon.height() as i32;

            let scale = f32::min(
                ICON_BTN_SIZE as f32 / iw.max(1) as f32,
                ICON_BTN_SIZE as f32 / ih.max(1) as f32,
            );
            let dw = ((iw as f32 * scale).round() as i32).max(1) as u32;
            let dh = ((ih as f32 * scale).round() as i32).max(1) as u32;

            // Resize into a temporary (avoid mutating original)
            let scaled = if dw == icon.width() && dh == icon.height() {
                icon.clone()
            } else {
                icon.resize(dw, dh, orbimage::ResizeType::Lanczos3).unwrap_or_else(|_| icon.clone())
            };

            let ix = x + (ACTIONS_SLOT_W - scaled.width() as i32) / 2;
            let iy = y + 6; // push icon towards the top inside the slot
            scaled.draw(win, ix, iy);
        }

        // Draw label centered under the icon
        let text_run = font.render(label, 14.0);
        let tx   = x + (ACTIONS_SLOT_W - text_run.width() as i32) / 2;
        // Place text so that icon + gap + text fits within slot height
        let ty   = y + ICON_BTN_SIZE + ICON_TEXT_GAP + 10;
        text_run.draw(win, tx, ty, LABEL);

        hits.push((act, rect));
        x += ACTIONS_SLOT_W + BTN_GAP;
    }

    hits
}

// draws text and avatar for user selection state
fn draw_select_state(
    font: &Font,
    win: &mut Window,
    usernames: &[String],
    hover: Option<usize>,
    mouse: Option<(i32, i32)>, // <-- added
) -> Vec<(usize, Rect)> {
    let w = win.width() as i32;
    let h = win.height() as i32;

    // clock
    draw_top_right_clock(font, win);

    let center_x = w / 2;
    let center_y = h / 2 - 40;

    // Determine which username the big avatar represents
    let selected_i = hover.unwrap_or(0).min(usernames.len().saturating_sub(1));

    // Recompute avatar and name hitboxes (same as in your event code)
    let avatar_rect = Rect::new(
        center_x - AVATAR_RADIUS,
        center_y - AVATAR_RADIUS,
        (AVATAR_RADIUS * 2) as u32,
        (AVATAR_RADIUS * 2) as u32,
    );
    let name_rect = Rect::new(center_x - 75, center_y + AVATAR_RADIUS + 8, 150, 24);

    // True hover is based on pointer position, not on the 'hover' index
    let over_avatar = if let Some((mx, my)) = mouse {
        avatar_rect.contains(mx, my) || name_rect.contains(mx, my)
    } else {
        false
    };

    // Avatar opacity: 85% normally, 100% on real hover
    let avatar_opacity: u8 = if over_avatar { 255 } else { 217 };
    draw_user_avatar_with_opacity(win, center_x, center_y, avatar_opacity);

    // Username label (brighten when pointer is over avatar/name)
    let name = &usernames[selected_i];
    let label = font.render(name, 18.0);
    let label_col = if over_avatar { LABEL } else { LABEL_D };
    label.draw(
        win,
        center_x - label.width() as i32 / 2,
        center_y + AVATAR_RADIUS + 8,
        label_col,
    );

    // Secondary row (list) with per-item hover based on 'hover' index
    let mut hit = Vec::new();
    if usernames.len() > 1 {
        let slot_w = 140;
        let gap = 16;
        let total = usernames.len() as i32 * slot_w + (usernames.len() as i32 - 1) * gap;
        let mut x = center_x - total / 2;
        let y = center_y + AVATAR_RADIUS + 32 + 8;

        for (i, u) in usernames.iter().enumerate() {
            let rect = Rect::new(x, y, slot_w as u32, 28);
            let hl = hover == Some(i);

            // subtle hover background
            win.rect(
                x, y, rect.w, rect.h,
                if hl { Color::rgba(255, 255, 255, 40) } else { Color::rgba(0, 0, 0, 0) }
            );

            // brighter text when hovered
            let r = font.render(u, 14.0);
            let text_col = if hl { LABEL } else { LABEL_D };
            r.draw(
                win,
                x + (slot_w - r.width() as i32) / 2,
                y + (28 - r.height() as i32) / 2,
                text_col,
            );

            hit.push((i, rect));
            x += slot_w + gap;
        }
    }
    hit
}

// draws avatar with password enter
fn draw_password_state(
    font: &Font,
    win: &mut Window,
    user: &str,
    pwd: &str,
    focus_pwd: bool,
    show_error: bool,
) -> (Rect, Rect) {
    let w = win.width() as i32;
    let h = win.height() as i32;

    // clock
    draw_top_right_clock(font, win);

    let center_x = w / 2;
    let center_y = h / 2 - 40;

    // Avatar always 100% in password state
    draw_user_avatar_with_opacity(win, center_x, center_y, 255);

    // Username label
    let name = font.render(user, 18.0);
    name.draw(
        win,
        center_x - name.width() as i32 / 2,
        center_y + AVATAR_RADIUS + 8,
        LABEL,
    );

    // ---- BACK BUTTON (round) ----
    let field_w = 360;
    let panel_y = center_y + AVATAR_RADIUS + 40;
    let panel_x = center_x - field_w / 2;

    let back_rect = Rect::new(panel_x - (FIELD_H + 8), panel_y, FIELD_H as u32, FIELD_H as u32);

    // White @ 35% (≈ alpha 90)
    let white35 = Color::rgba(255, 255, 255, 90);

    // Round back button
    let back_cx = back_rect.x + back_rect.w as i32 / 2;
    let back_cy = back_rect.y + back_rect.h as i32 / 2;
    let back_r  = (back_rect.w.min(back_rect.h) as i32) / 2;
    fill_circle(win, back_cx, back_cy, back_r, white35);

    // Icon (image or fallback glyph)
    if let Ok(back_img) = Image::from_path("/ui/icons/actions/back.png") {
        draw_image_centered(win, &back_img, back_rect);
    } else {
        let arr = font.render("<", 18.0);
        arr.draw(
            win,
            back_rect.x + (FIELD_H - arr.width() as i32) / 2,
            back_rect.y + (FIELD_H - arr.height() as i32) / 2,
            LABEL,
        );
    }

    // ---- PASSWORD FIELD (rounded corners: 10px) ----
    let field_rect = Rect::new(panel_x, panel_y, field_w as u32, FIELD_H as u32);
    fill_round_rect(win, field_rect, 10, white35);

    // Placeholder / dots
    let shown = if pwd.is_empty() { "Enter Password" } else { &"•".repeat(pwd.chars().count()) };
    let color = if show_error { ERROR } else if focus_pwd { LABEL } else { LABEL_D };
    let r = font.render(shown, 16.0);
    r.draw(
        win,
        field_rect.x + PANEL_PAD,
        field_rect.y + (FIELD_H - r.height() as i32) / 2,
        color,
    );

    // Optional error underline
    if show_error {
        win.rect(field_rect.x, field_rect.y + FIELD_H - 2, field_rect.w, 2, ERROR);
    }

    (back_rect, field_rect)
}

fn get_actions_hitboxes(
    win: &mut Window,
    y: i32,
    state_is_pwd: bool,
    icons: &ActionIcons,
) -> Vec<(Action, Rect)> {
    // gleiche Reihenfolge wie in draw_actions_bar
    let mut items: Vec<(Action, &str, Option<&Image>)> = vec![
        (Action::Sleep,    "Sleep",    icons.sleep.as_ref()),
        (Action::Restart,  "Restart",  icons.restart.as_ref()),
        (Action::Shutdown, "Shutdown", icons.shutdown.as_ref()),
    ];
    if state_is_pwd {
        items.push((Action::Logout, "Logout", icons.logout.as_ref()));
    }

    let w = win.width() as i32;
    let n = items.len() as i32;
    let total_w = n * ACTIONS_SLOT_W + (n - 1) * BTN_GAP;
    let mut x = (w - total_w) / 2;

    let mut hits = Vec::with_capacity(items.len());
    for (act, _label, _icon_opt) in items {
        let rect = Rect::new(x, y, ACTIONS_SLOT_W as u32, ACTIONS_SLOT_H as u32);
        hits.push((act, rect));
        x += ACTIONS_SLOT_W + BTN_GAP;
    }
    hits
}

fn get_password_hitboxes(
    win: &mut Window,
    _user: &str,
    _pwd: &str,
    _focus_pwd: bool,
    _show_error: bool,
) -> (Rect, Rect) {
    let w = win.width() as i32;
    let h = win.height() as i32;

    let center_x = w/2;
    let center_y = h/2 - 40;

    let field_w = 360;
    let panel_y = center_y + AVATAR_RADIUS + 40;
    let panel_x = center_x - field_w/2;

    let back_rect  = Rect::new(panel_x - (FIELD_H + 8), panel_y, FIELD_H as u32, FIELD_H as u32);
    let field_rect = Rect::new(panel_x, panel_y, field_w as u32, FIELD_H as u32);
    (back_rect, field_rect)
}

// Draws the avatar with a simulated opacity effect.
// We render the avatar normally, then if opacity < 100%,
// we darken it by overlaying a semi-transparent black circle.
fn draw_user_avatar_with_opacity(win: &mut Window, center_x: i32, center_y: i32, opacity: u8) {
    let size = (AVATAR_RADIUS * 2) as u32;
    let rect = Rect::new(center_x - AVATAR_RADIUS, center_y - AVATAR_RADIUS, size, size);

    if let Ok(img) = Image::from_path("/ui/icons/system/avatar.png") {
        draw_image_centered(win, &img, rect);
    } else {
        // fallback rectangle if the asset is missing
        win.rect(rect.x, rect.y, rect.w, rect.h, Color::rgba(255,255,255,32));
    }

    // Apply overlay mask if opacity < 100%
    if opacity < 255 {
        let overlay_alpha = 255u16.saturating_sub(opacity as u16) as u8;
        let cx = rect.x + rect.w as i32 / 2;
        let cy = rect.y + rect.h as i32 / 2;
        fill_circle(win, cx, cy, AVATAR_RADIUS, Color::rgba(0, 0, 0, overlay_alpha));
    }
}

fn fill_circle(win: &mut Window, cx: i32, cy: i32, r: i32, color: Color) {
    // Rasterized filled circle via horizontal scanlines
    for dy in -r..=r {
        let dx = (((r * r) - (dy * dy)) as f32).sqrt() as i32;
        let x = cx - dx;
        let y = cy + dy;
        win.rect(x, y, (dx * 2 + 1) as u32, 1, color);
    }
}

fn fill_round_rect(win: &mut Window, rect: Rect, radius: i32, color: Color) {
    // clamp radius
    let r = radius.min(rect.w.min(rect.h) as i32 / 2).max(0);

    // center area (without the rounded corners)
    let x = rect.x;
    let y = rect.y;
    let w = rect.w as i32;
    let h = rect.h as i32;

    // middle rectangle
    win.rect(x + r, y, (w - 2 * r) as u32, h as u32, color);

    // left & right rectangles (between the corner arcs)
    if r > 0 {
        win.rect(x,         y + r, r as u32,        (h - 2 * r) as u32, color);
        win.rect(x + w - r, y + r, r as u32,        (h - 2 * r) as u32, color);

        // four quarter-circles
        for dy in 0..r {
            let dx = (((r * r) - (dy * dy)) as f32).sqrt() as i32;

            // top-left
            win.rect(x + r - dx,       y + r - 1 - dy, (dx) as u32, 1, color);
            // top-right
            win.rect(x + w - r,        y + r - 1 - dy, (dx) as u32, 1, color);
            // bottom-left
            win.rect(x + r - dx,       y + h - r + dy, (dx) as u32, 1, color);
            // bottom-right
            win.rect(x + w - r,        y + h - r + dy, (dx) as u32, 1, color);
        }
    }
}

fn main() -> io::Result<()> {
    // Initialize logging (ignore errors if logging setup fails)
    let _ = RedoxLogger::new()
        .with_output(
            OutputBuilder::stdout()
                .with_filter(log::LevelFilter::Debug)
                .with_ansi_escape_codes()
                .build(),
        )
        .with_process_name("nexus-login".into())
        .enable();
    log::info!("*** NEXUS_Login ACTIVE ***");

    // Collect launcher command + args from arguments
    let mut args = env::args().skip(1);
    let launcher_cmd = args.next().ok_or(io::Error::new(
        io::ErrorKind::Other,
        "Could not get 'launcher_cmd'",
    ))?;
    let launcher_args: Vec<String> = args.collect();

    // Main login loop: show login, start session, then return to login on logout
    loop {
        match login_window(&launcher_cmd, &launcher_args) {
            Ok(Some(mut command)) => {
                // Spawn user session and wait until it exits
                match command.spawn() {
                    Ok(mut child) => {
                        if let Err(e) = child.wait() {
                            error!("failed to wait for '{}': {}", launcher_cmd, e);
                        }
                        // After session exit, loop restarts and login_window is shown again
                    }
                    Err(e) => {
                        error!("failed to exec '{}': {}", launcher_cmd, e);
                        // Continue loop to retry login
                    }
                }
            }
            Ok(None) => {
                // User cancelled or window quit → end login process
                return Ok(());
            }
            Err(e) => {
                error!("{}", e);
                return Err(io::Error::new(io::ErrorKind::Other, e));
            }
        }
    }
}

