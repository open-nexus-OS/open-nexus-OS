// src/ui/bar_handler.rs
// Bottom taskbar logic — fixed event subscriptions and modularized services integration.
// Immediate toggle behavior (no animations), ActionBar + Start menu working again.

use std::io::{self, ErrorKind, Read, Write};
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::process::{Child, Command};
use std::{mem};
use log::{debug, error, info};

use orbclient::{EventOption, Renderer, Window, WindowFlag};
use orbfont::Font;
use orbimage::Image;

use event::{user_data, EventQueue};
use libredox::data::TimeSpec;
use libredox::flag;

use crate::ui::bar_msg;
use super::bar_msg::{apply_initial_settings, handle_bar_msg};

use crate::dpi_scale;
use crate::config::colors::{
    bar_paint, bar_highlight_paint, bar_activity_marker_paint,
    text_paint, text_highlight_paint, load_crisp_font,
};
use crate::config::settings::{BAR_HEIGHT, ICON_SCALE, ICON_SMALL_SCALE, Mode, mode};
use crate::utils::dpi_helper as dpi_helper;
use crate::ui::menu_handler::{MenuHandler, MenuResult};
use crate::modes::desktop::{show_desktop_menu, DesktopMenuResult};
use crate::modes::mobile::{show_mobile_menu, MobileMenuResult};
use crate::services::process_manager::{exec_to_command, wait};
use crate::services::app_catalog::{
    get_packages, organize_packages_by_category, load_start_icon,
};
use crate::services::package_service::{IconSource, Package};
use crate::ui::actionbar_handler::ActionBarHandler;

use std::collections::BTreeMap;

/// Convenience: bar icon sizes (derived from settings)
fn icon_size() -> i32 { (BAR_HEIGHT as f32 * ICON_SCALE).round() as i32 }
fn icon_small_size() -> i32 { (icon_size() as f32 * ICON_SMALL_SCALE).round() as i32 }
fn font_size() -> i32 { (icon_size() as f32 * 0.5).round() as i32 }

/// Simple chooser list (legacy) – used in category fallback
fn draw_chooser(window: &mut Window, font: &Font, packages: &mut Vec<Package>, selected: i32) {
    let w = window.width();
    window.set(bar_paint().color);

    let dpi = dpi_scale();
    let target_icon = icon_small_size().max(20) as u32;

    let mut y = 0;
    for (i, package) in packages.iter_mut().enumerate() {
        if i as i32 == selected {
            window.rect(0, y, w, target_icon as u32, bar_highlight_paint().color);
        }

        let img = package.get_icon_sized(target_icon, dpi, false);
        img.draw(window, 0, y);

        font.render(&package.name, dpi_helper::font_size(font_size() as f32).round()).draw(
            window,
            target_icon as i32 + 8,
            y + 8,
            if i as i32 == selected { text_highlight_paint().color } else { text_paint().color },
        );

        y += target_icon as i32;
    }

    window.sync();
}

struct Bar {
    children: Vec<(String, Child)>,
    packages: Vec<Package>,
    start: Image,
    start_packages: Vec<Package>,
    category_packages: BTreeMap<String, Vec<Package>>,
    font: Font,
    width: u32,
    height: u32,
    window: Window,
    selected: i32,
    selected_window: Window,
    time: String,
}

impl Bar {
    fn new(width: u32, height: u32) -> Bar {
        // Load & organize packages using the catalog service
        let all = get_packages();
        let (root_packages, category_packages, start_packages) = organize_packages_by_category(all);

        // Start icon via theme (with PNG fallback handled inside)
        let start = load_start_icon();

        Bar {
            children: Vec::new(),
            packages: root_packages,
            start,
            start_packages,
            category_packages,
            font: load_crisp_font(),
            width,
            height,
            window: Window::new_flags(
                0,
                height as i32 - BAR_HEIGHT as i32,
                width,
                BAR_HEIGHT,
                "NexusLauncherBar",
                &[WindowFlag::Async, WindowFlag::Borderless, WindowFlag::Transparent],
            ).expect("launcher: failed to open bar window"),
            selected: -1,
            selected_window: Window::new_flags(
                0,
                height as i32,
                width,
                (font_size() + 8) as u32,
                "NexusLauncherTip",
                &[WindowFlag::Async, WindowFlag::Borderless, WindowFlag::Transparent],
            ).expect("launcher: failed to open tip window"),
            time: String::new(),
        }
    }

    fn update_time(&mut self) {
        let time = libredox::call::clock_gettime(flag::CLOCK_REALTIME)
            .expect("launcher: failed to read time");
        let ts = time.tv_sec;
        let s = ts % 86400;
        let h = s / 3600;
        let m = s / 60 % 60;
        self.time = format!("{:>02}:{:>02}", h, m)
    }

    fn draw(&mut self) {
        self.window.set(bar_paint().color);

        let bar_h_u = self.window.height();
        let bar_h_i = bar_h_u as i32;
        let slot = bar_h_i;

        let count = 1 + self.packages.len() as i32;
        let total_w = count * slot;

        let mut x = (self.width as i32 - total_w) / 2;
        let mut i = 0i32;

        // Start icon
        {
            if i == self.selected {
                self.window.rect(x, 0, slot as u32, bar_h_u, bar_highlight_paint().color);
            }
            let y = (bar_h_i - self.start.height() as i32) / 2;
            let ix = x + (slot - self.start.width() as i32) / 2;
            self.start.draw(&mut self.window, ix, y);

            x += slot;
            i += 1;
        }

        // App icons
        let dpi = dpi_scale();
        let target_icon = icon_size() as u32;
        for package in self.packages.iter_mut() {
            if i == self.selected {
                self.window.rect(x, 0, slot as u32, bar_h_u, bar_highlight_paint().color);

                self.selected_window.set(orbclient::Color::rgba(0, 0, 0, 0));
                let text = self.font.render(&package.name, dpi_helper::font_size(font_size() as f32).round());
                self.selected_window
                    .rect(x, 0, text.width() + 8, text.height() + 8, bar_paint().color);
                text.draw(&mut self.selected_window, x + 4, 4, text_highlight_paint().color);
                self.selected_window.sync();
                let sw_y = self.window.y() - self.selected_window.height() as i32 - 4;
                self.selected_window.set_pos(0, sw_y);
            }

            let image = package.get_icon_sized(target_icon, dpi, false);

            let y = (bar_h_i - image.height() as i32) / 2;
            let ix = x + (slot - image.width() as i32) / 2;
            image.draw(&mut self.window, ix, y);

            // Activity marker if child process is alive
            let running = self.children.iter().any(|(exec, _)| exec == &package.exec);
            if running {
                let inset = 4i32;
                let marker_x = x + inset;
                let marker_w = (slot - 2 * inset).max(2) as u32;
                self.window.rect(marker_x, 0, marker_w, 4, bar_activity_marker_paint().color);
            }

            x += slot;
            i += 1;
        }

        // Clock (right)
        let text = self.font.render(&self.time, dpi_helper::font_size((font_size() * 2) as f32).round());
        let tx = self.width as i32 - text.width() as i32 - 8;
        let ty = (bar_h_i - text.height() as i32) / 2;
        text.draw(&mut self.window, tx, ty, text_highlight_paint().color);

        self.window.sync();
    }

    /// Legacy small category chooser if needed
    fn start_window_legacy(&mut self, category_opt: Option<&String>) -> Option<String> {
        // Delegate to new menus when no category given
        if category_opt.is_none() {
            match mode() {
                Mode::Desktop => match show_desktop_menu(self.width, self.height, &mut self.packages) {
                    DesktopMenuResult::Launch(exec) => return Some(exec),
                    _ => return None,
                },
                Mode::Mobile  => match show_mobile_menu(self.width, self.height, &mut self.packages) {
                    MobileMenuResult::Launch(exec) => return Some(exec),
                    _ => return None,
                }
            }
        }

        // Small chooser (fallback)
        let packages = match category_opt {
            Some(category) => self.category_packages.get_mut(category)?,
            None => &mut self.start_packages,
        };

        let dpi = dpi_scale();
        let target = icon_small_size() as u32;

        let start_h = packages.len() as u32 * target;
        let mut start_window = Window::new_flags(
            0,
            self.height as i32 - icon_size() - start_h as i32,
            200,
            start_h,
            "Start",
            &[WindowFlag::Borderless, WindowFlag::Transparent],
        ).unwrap();

        let mut selected = -1;
        let mut mouse_y = 0;
        let mut mouse_left = false;
        let mut last_mouse_left = false;

        // initial draw
        start_window.set(bar_paint().color);
        {
            let mut y = 0;
            for (i, p) in packages.iter_mut().enumerate() {
                if i as i32 == selected {
                    start_window.rect(0, y, 200, target, bar_highlight_paint().color);
                }
                let img = p.get_icon_sized(target, dpi, false);
                img.draw(&mut start_window, 0, y);
                let text = self.font.render(&p.name, dpi_helper::font_size(font_size() as f32).round());
                text.draw(&mut start_window, target as i32 + 8, y + 8, text_paint().color);
                y += target as i32;
            }
            start_window.sync();
        }

        'start_choosing: loop {
            for event in start_window.events() {
                let redraw = match event.to_option() {
                    EventOption::Mouse(mouse_event) => { mouse_y = mouse_event.y; true }
                    EventOption::Button(button_event) => { mouse_left = button_event.left; true }
                    EventOption::Quit(_) => break 'start_choosing None,
                    _ => false,
                };

                if redraw {
                    let mut now_selected = -1;

                    let mut y = 0;
                    for (j, _package) in packages.iter().enumerate() {
                        if mouse_y >= y && mouse_y < y + icon_small_size() {
                            now_selected = j as i32;
                        }
                        y += icon_small_size();
                    }

                    if now_selected != selected {
                        selected = now_selected;
                        draw_chooser(&mut start_window, &self.font, packages, selected);
                    }

                    if !mouse_left && last_mouse_left {
                        let mut y = 0;
                        for package_i in 0..packages.len() {
                            if mouse_y >= y && mouse_y < y + icon_small_size() {
                                return Some(packages[package_i].exec.to_string());
                            }
                            y += icon_small_size();
                        }
                    }

                    last_mouse_left = mouse_left;
                }
            }
        }
    }

    fn spawn(&mut self, exec: String) {
        match exec_to_command(&exec, None) {
            Some(mut command) => match command.spawn() {
                Ok(child) => {
                    self.children.push((exec, child));
                    self.draw();
                }
                Err(err) => error!("failed to spawn {}: {}", exec, err),
            },
            None => error!("failed to parse {}", exec),
        }
    }
}

pub fn bar_main(width: u32, height: u32) -> io::Result<()> {
    let mut bar = Bar::new(width, height);
    apply_initial_settings();

    // Start background
    match Command::new("nexus-background").spawn() {
        Ok(child) => bar.children.push(("nexus-background".to_string(), child)),
        Err(err) => error!("failed to launch nexus-background: {}", err),
    }

    // ActionBar handler (top bar + panels), immediate toggle
    let mut actionbar = ActionBarHandler::new(width, height);
    actionbar.initialize(width);

    // --- Event system setup ---
    user_data! { enum Ev { Time, Bar, ActBar, Panels } }
    let queue = EventQueue::<Ev>::new().expect("launcher: failed to create event queue");

    // Timer FD
    let mut time_file = File::open("/scheme/time/4")?;
    let mut time_buf = [0_u8; core::mem::size_of::<TimeSpec>()];
    if let time = libredox::data::timespec_from_mut_bytes(&mut time_buf) {
        time.tv_sec += 1;
        time.tv_nsec = 0;
    }
    time_file.write(&time_buf)?;

    // Subscribe: timer + bar window + actionbar + panels
    queue.subscribe(time_file.as_raw_fd() as usize, Ev::Time, event::EventFlags::READ)?;
    queue.subscribe(bar.window.as_raw_fd() as usize, Ev::Bar, event::EventFlags::READ)?;

    let actionbar_fd = actionbar.get_actionbar_fd();
    if actionbar_fd >= 0 {
        queue.subscribe(actionbar_fd as usize, Ev::ActBar, event::EventFlags::READ)?;
    }
    let panels_fd = actionbar.get_panels_fd();
    if panels_fd >= 0 {
        queue.subscribe(panels_fd as usize, Ev::Panels, event::EventFlags::READ)?;
    }

    let mut mouse_x = -1;
    let mut mouse_y = -1;
    let mut mouse_left = false;
    let mut last_mouse_left = false;

    let mut menu = MenuHandler::new();

    'events: for evt in queue.map(|e| e.expect("next event")) {
        match evt.user_data {
            Ev::Time => {
                // Prime next tick first to avoid missing edges on errors
                if time_file.read(&mut time_buf)? < mem::size_of::<TimeSpec>() { continue; }
                if let time = libredox::data::timespec_from_mut_bytes(&mut time_buf) {
                    time.tv_sec += 1;
                    time.tv_nsec = 0;
                }
                time_file.write(&time_buf)?;

                // Reap children (non-blocking)
                // (keine lokale Liste hier – Bar::spawn reaps via try_wait in draw loop wäre auch ok)
                let mut status = 0;
                loop {
                    let pid = wait(&mut status)?;
                    if pid == 0 { break; }
                }

                // Update & redraw bar clock
                bar.update_time();
                bar.draw();

                // Keep ActionBar panels visibility in sync (immediate)
                actionbar.render_now(bar.width, bar.height);
            }

            Ev::ActBar => {
                // Process ActionBar window events
                if let Some(msg) = actionbar.process_events(bar.width, bar.height) {
                    match handle_bar_msg(msg) {
                    crate::ui::bar_msg::AppliedMsg::ModeChanged => {
                    // Reposition/hide bottom bar according to current mode
                    match crate::config::settings::mode() {
                    crate::config::settings::Mode::Mobile => {
                    // Hide desktop bar in Mobile (or adjust as you see fit)
                    bar.window.set_pos(0, bar.height as i32); // off-screen
                    bar.selected_window.set_pos(0, bar.height as i32);
                                }
                                crate::config::settings::Mode::Desktop => {
                    // Show desktop bar at bottom
                                    bar.window.set_pos(0, bar.height as i32 - icon_size());
                                    bar.selected_window.set_pos(0, bar.height as i32);
                                }
                            }
                            // Repaint + refresh panels (bottom gap changes)
                            bar.draw();
                            actionbar.render_now(bar.width, bar.height);
                        }
                        _ => { /* no-op */ }
                    }
                }
            }

            Ev::Panels => {
                // Panels usually don't need heavy event handling; we let ActionBar handler process if any.
                if let Some(msg) = actionbar.process_events(bar.width, bar.height) {
                    match handle_bar_msg(msg) {
                    crate::ui::bar_msg::AppliedMsg::ModeChanged => {
                    match crate::config::settings::mode() {
                    crate::config::settings::Mode::Mobile => {
                                    bar.window.set_pos(0, bar.height as i32);
                                    bar.selected_window.set_pos(0, bar.height as i32);
                                }
                    crate::config::settings::Mode::Desktop => {
                                    bar.window.set_pos(0, bar.height as i32 - icon_size());
                                    bar.selected_window.set_pos(0, bar.height as i32);
                                }
                            }
                            bar.draw();
                            actionbar.render_now(bar.width, bar.height);
                        }
                    _ => { /* no-op */ }
                    }
                }
            }

            Ev::Bar => {
                for ev in bar.window.events() {
                    // Handle Super+Hotkeys (unchanged)
                    if ev.code >= 0x1000_0000 {
                        let mut s_ev = ev;
                        s_ev.code -= 0x1000_0000;
                        if let EventOption::Key(k) = s_ev.to_option() {
                            if k.pressed {
                                match k.scancode {
                                    orbclient::K_B => bar.spawn("netsurf-fb".to_string()),
                                    orbclient::K_F => bar.spawn("cosmic-files".to_string()),
                                    orbclient::K_T => bar.spawn("cosmic-term".to_string()),
                                    _ => {}
                                }
                            }
                        }
                        continue;
                    }

                    let redraw = match ev.to_option() {
                        EventOption::Mouse(m) => { mouse_x = m.x; mouse_y = m.y; true }
                        EventOption::Button(b) => {
                            if b.left { menu.reset_suppress_on_click(); }
                            mouse_left = b.left; true
                        }
                        EventOption::Screen(s) => {
                            // Resize bar windows
                            bar.width  = s.width;
                            bar.height = s.height;
                            bar.window.set_pos(0, s.height as i32 - icon_size());
                            bar.window.set_size(s.width, icon_size() as u32);
                            bar.selected = -2; // force redraw
                            bar.selected_window.set_pos(0, s.height as i32);
                            bar.selected_window.set_size(s.width, (font_size() + 8) as u32);

                            // Resize ActionBar (top inset handled inside)
                            actionbar.handle_screen_resize(s.width, s.height);
                            true
                        }
                        EventOption::Hover(h) => {
                            if h.entered { false } else { mouse_x = -1; mouse_y = -1; true }
                        }
                        EventOption::Quit(_) => break 'events,
                        _ => false,
                    };

                    if redraw {
                        // Slot hit-testing
                        let mut now_selected = -1;
                        {
                            let slot  = bar.window.height() as i32;
                            let count = 1 + bar.packages.len() as i32;
                            let total = count * slot;
                            let mut x = (bar.width as i32 - total) / 2;
                            let y = 0;
                            let mut i = 0;

                            // Start
                            if mouse_y >= y && mouse_x >= x && mouse_x < x + slot { now_selected = i; }
                            x += slot; i += 1;

                            // Apps
                            for _ in bar.packages.iter() {
                                if mouse_y >= y && mouse_x >= x && mouse_x < x + slot { now_selected = i; }
                                x += slot; i += 1;
                            }
                        }

                        if now_selected != bar.selected {
                            bar.selected = now_selected;
                            let sw_y = bar.height as i32;
                            bar.selected_window.set_pos(0, sw_y);
                            bar.draw();
                        }

                        // Release edge: clicks
                        if !mouse_left && last_mouse_left {
                            let mut i = 0;

                            // Start button slot
                            if i == bar.selected {
                                // Dismiss ActionBar panels first
                                actionbar.dismiss_panels();

                                // Start menu handling (desktop/mobile)
                                match menu.handle_start_menu_click(bar.width, bar.height, &mut bar.packages) {
                                    MenuResult::Launch(exec) => { if !exec.trim().is_empty() { bar.spawn(exec); } }
                                    MenuResult::Logout       => break 'events,
                                    MenuResult::None         => {}
                                }
                            }
                            i += 1;

                            // App slots
                            for package_i in 0..bar.packages.len() {
                                if i == bar.selected {
                                    let exec = bar.packages[package_i].exec.clone();
                                    if exec.trim().is_empty() {
                                        log::warn!("selected package has empty exec, skipping");
                                    } else {
                                        bar.spawn(exec);
                                    }
                                }
                                i += 1;
                            }
                        }

                        last_mouse_left = mouse_left;
                    }
                }
            }
        }
    }

    // Cleanup children
    for (exec, child) in bar.children.iter_mut() {
        let pid = child.id();
        match child.kill() {
            Ok(()) => debug!("killed child: {}", pid),
            Err(err) => error!("failed to kill {} ({}): {}", exec, pid, err),
        }
        let _ = child.wait();
    }

    Ok(())
}
