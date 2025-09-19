// src/main.rs
// Launcher entry: taskbar ("bar"), start menu (desktop/mobile), chooser.
// Updated to use themed, sized icons via Package::get_icon_sized.
// Removed legacy image() usage and ensures crisp icons across toggles.

extern crate event;
extern crate freedesktop_entry_parser;
extern crate libredox;
extern crate log;
extern crate orbclient;
extern crate orbfont;
extern crate orbimage;
extern crate redox_log;

use event::{user_data, EventQueue};
use libredox::data::TimeSpec;
use libredox::flag;
use log::{debug, error, info};
use redox_log::{OutputBuilder, RedoxLogger};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{ErrorKind, Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::{env, io, mem};

use orbclient::{EventOption, Renderer, Window, WindowFlag};
use orbfont::Font;
use orbimage::Image;

use package::{IconSource, Package};
use config::{bar_paint, bar_highlight_paint, bar_activity_marker_paint, text_paint, text_highlight_paint, BAR_HEIGHT, ICON_SCALE, ICON_SMALL_SCALE};

use libnexus::themes::{THEME, IconVariant};

pub mod modes {
    pub mod desktop;
    pub mod mobile;
}
mod package;
mod ui;
mod icons;
mod config;
mod helper {
    pub mod dpi_helper;
}

static SCALE: AtomicIsize = AtomicIsize::new(1);

/// Get DPI scale factor using the helper module
pub fn dpi_scale() -> f32 {
    helper::dpi_helper::get_dpi_scale()
}

fn chooser_width() -> u32 {
    200 * SCALE.load(Ordering::Relaxed) as u32
}

fn icon_size() -> i32 {
    (BAR_HEIGHT as f32 * ICON_SCALE).round() as i32
}

fn icon_small_size() -> i32 {
    (icon_size() as f32 * ICON_SMALL_SCALE).round() as i32
}

fn font_size() -> i32 {
    (icon_size() as f32 * 0.5).round() as i32
}

#[cfg(target_os = "redox")]
static UI_PATH: &'static str = "/ui";
#[cfg(not(target_os = "redox"))]
static UI_PATH: &'static str = "ui";

// Legacy helpers kept only for local PNGs (e.g. start-here as fallback).
fn size_icon(icon: orbimage::Image, target: u32) -> orbimage::Image {
    if icon.width() == target && icon.height() == target {
        return icon;
    }
    icon.resize(target, target, orbimage::ResizeType::Lanczos3).unwrap()
}

fn load_png<P: AsRef<Path>>(path: P, target: u32) -> Image {
    let icon = Image::from_path(path).unwrap_or(Image::default());
    size_icon(icon, target)
}

fn get_packages() -> Vec<Package> {
    let mut packages: Vec<Package> = Vec::new();

    if let Ok(read_dir) = Path::new(&format!("{}/apps/", UI_PATH)).read_dir() {
        for entry_res in read_dir {
            let entry = match entry_res {
                Ok(x) => x,
                Err(_) => continue,
            };
            if entry
                .file_type()
                .expect("failed to get file_type")
                .is_file()
            {
                packages.push(Package::from_path(&entry.path().display().to_string()));
            }
        }
    }

    if let Ok(xdg_dirs) = xdg::BaseDirectories::new() {
        for path in xdg_dirs.find_data_files("applications") {
            if let Ok(read_dir) = path.read_dir() {
                for dir_entry_res in read_dir {
                    let Ok(dir_entry) = dir_entry_res else { continue; };
                    let Ok(id) = dir_entry.file_name().into_string() else { continue; };
                    if let Some(package) = Package::from_desktop_entry(id, &dir_entry.path()) {
                        packages.push(package);
                    }
                }
            }
        }
    }

    packages.sort_by(|a, b| a.name.cmp(&b.name));
    packages
}

/// Simple chooser list for file-open scenarios (small icons)
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

        font.render(&package.name, font_size() as f32).draw(
            window,
            target_icon as i32 + 8,
            y + 8,
            if i as i32 == selected {
                text_highlight_paint().color
            } else {
                text_paint().color
            },
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
    start_menu_open: bool,
    suppress_start_open: bool,
}

impl Bar {
    fn new(width: u32, height: u32) -> Bar {
        let all_packages = get_packages();

        // Split packages by category
        let mut root_packages = Vec::new();
        let mut category_packages = BTreeMap::<String, Vec<Package>>::new();
        for package in all_packages {
            if package.categories.is_empty() {
                root_packages.push(package);
            } else {
                for category in package.categories.iter() {
                    match category_packages.get_mut(category) {
                        Some(vec) => vec.push(package.clone()),
                        None => {
                            category_packages.insert(category.clone(), vec![package.clone()]);
                        }
                    }
                }
            }
        }

        root_packages.sort_by(|a, b| a.id.cmp(&b.id));
        root_packages.retain(|p| !p.exec.trim().is_empty());

        let mut start_packages = Vec::new();

        // Category launchers in start menu — use logical icon ids (theme-managed)
        for (category, packages) in category_packages.iter_mut() {
            start_packages.push({
                let mut package = Package::new();
                package.name = category.to_string();
                package.icon.source = IconSource::Name("mimetypes/inode-directory".into());
                package.icon_small.source = IconSource::Name("mimetypes/inode-directory".into());
                package.exec = format!("category={}", category);
                package
            });

            packages.push({
                let mut package = Package::new();
                package.name = "Go back".to_string();
                package.icon.source = IconSource::Name("mimetypes/inode-directory".into());
                package.icon_small.source = IconSource::Name("mimetypes/inode-directory".into());
                package.exec = "exit".to_string();
                package
            });
        }

        start_packages.push({
            let mut package = Package::new();
            package.name = "Logout".to_string();
            package.icon.source = IconSource::Name("actions/system-log-out".into());
            package.icon_small.source = IconSource::Name("actions/system-log-out".into());
            package.exec = "exit".to_string();
            package
        });

        // Start icon on the bar (theme-sized to BAR_HEIGHT)
        let start_size = BAR_HEIGHT.max(24);
        let start = THEME
            .load_icon_sized("places/start-here", IconVariant::Auto, Some((start_size, start_size)))
            .unwrap_or_else(|| load_png(format!("{}/icons/places/start-here.png", UI_PATH), start_size));

        Bar {
            children: Vec::new(),
            packages: root_packages,
            start,
            start_packages,
            category_packages,
            font: Font::find(Some("Sans"), None, None).unwrap(),
            width,
            height,
            window: Window::new_flags(
                0,
                height as i32 - BAR_HEIGHT as i32,
                width,
                BAR_HEIGHT,
                "",
                &[
                    WindowFlag::Async,
                    WindowFlag::Borderless,
                    WindowFlag::Transparent,
                ],
            )
            .expect("launcher: failed to open window"),
            selected: -1,
            selected_window: Window::new_flags(
                0,
                height as i32,
                width,
                (font_size() + 8) as u32,
                "",
                &[
                    WindowFlag::Async,
                    WindowFlag::Borderless,
                    WindowFlag::Transparent,
                ],
            )
            .expect("launcher: failed to open selected window"),
            time: String::new(),
            start_menu_open: false,
            suppress_start_open: false,
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

        // Start icon slot
        {
            if i == self.selected {
                self.window.rect(x, 0, slot as u32, bar_h_u, bar_highlight_paint().color);
            }

            // Draw start icon centered in slot
            let y  = (bar_h_i - self.start.height() as i32) / 2;
            let ix = x + (slot - self.start.width() as i32) / 2;
            self.start.draw(&mut self.window, ix, y);

            x += slot;
            i += 1;
        }

        // App slots
        let dpi = dpi_scale();
        let target_icon = slot as u32 - 6; // a bit of padding in slot
        for package in self.packages.iter_mut() {
            if i == self.selected {
                self.window.rect(x, 0, slot as u32, bar_h_u, bar_highlight_paint().color);

                self.selected_window.set(orbclient::Color::rgba(0, 0, 0, 0));
                let text = self.font.render(&package.name, font_size() as f32);
                self.selected_window
                    .rect(x, 0, text.width() + 8, text.height() + 8, bar_paint().color);
                text.draw(&mut self.selected_window, x + 4, 4, text_highlight_paint().color);
                self.selected_window.sync();
                let sw_y = self.window.y() - self.selected_window.height() as i32 - 4;
                self.selected_window.set_pos(0, sw_y);
            }

            // Get a small (bar) icon at crisp size
            let image = package.get_icon_sized(target_icon, dpi, false);

            let y  = (bar_h_i - image.height() as i32) / 2;
            let ix = x + (slot - image.width() as i32) / 2;
            image.draw(&mut self.window, ix, y);

            // Activity marker
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

        // Clock right
        let text = self.font.render(&self.time, (font_size() * 2) as f32);
        let tx = self.width as i32 - text.width() as i32 - 8;
        let ty = (bar_h_i - text.height() as i32) / 2;
        text.draw(&mut self.window, tx, ty, text_highlight_paint().color);

        self.window.sync();
    }

    fn start_window(&mut self, category_opt: Option<&String>) -> Option<String> {
        use orbclient::{EventOption, Window, WindowFlag, K_ESC};

        // New path: delegate to desktop/mobile menus
        if category_opt.is_none() {
            match crate::config::mode() {
                crate::config::Mode::Desktop => {
                    match crate::modes::desktop::show_desktop_menu(self.width, self.height, &mut self.packages) {
                        crate::modes::desktop::DesktopMenuResult::Launch(exec) => return Some(exec),
                        _ => return None,
                    }
                }
                crate::config::Mode::Mobile => {
                    match crate::modes::mobile::show_mobile_menu(self.width, self.height, &mut self.packages) {
                        crate::modes::mobile::MobileMenuResult::Launch(exec) => return Some(exec),
                        _ => return None,
                    }
                }
            }
        }

        // Legacy small category chooser (kept only for nested categories)
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
            chooser_width(),
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
                    start_window.rect(0, y, chooser_width(), target, bar_highlight_paint().color);
                }
                let img = p.get_icon_sized(target, dpi, false);
                img.draw(&mut start_window, 0, y);
                let text = self.font.render(&p.name, font_size() as f32);
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
                    EventOption::Key(key_event) if key_event.scancode == K_ESC => break 'start_choosing,
                    EventOption::Focus(focus_event) => { if !focus_event.focused { break 'start_choosing; } false }
                    EventOption::Quit(_) => break 'start_choosing,
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

        None
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

fn exec_to_command(exec: &str, path_opt: Option<&str>) -> Option<Command> {
    let args_vec: Vec<String> = shlex::split(exec)?;
    let mut args = args_vec.iter();
    let mut command = Command::new(args.next()?);
    for arg in args {
        if arg.starts_with('%') {
            match arg.as_str() {
                "%f" | "%F" | "%u" | "%U" => {
                    if let Some(path) = &path_opt { command.arg(path); }
                }
                _ => {
                    log::warn!("unsupported Exec code {:?} in {:?}", arg, exec);
                    return None;
                }
            }
        } else {
            command.arg(arg);
        }
    }
    Some(command)
}

fn spawn_exec(exec: &str, path_opt: Option<&str>) {
    match exec_to_command(exec, path_opt) {
        Some(mut command) => {
            if let Err(err) = command.spawn() {
                error!("failed to launch {}: {}", exec, err);
            }
        }
        None => error!("failed to parse {}", exec),
    }
}

#[cfg(not(target_os = "redox"))]
fn wait(status: &mut i32) -> io::Result<usize> {
    extern crate libc;
    use std::io::Error;
    let pid = unsafe { libc::waitpid(0, status as *mut i32, libc::WNOHANG) };
    if pid < 0 {
        Err(io::Error::new(ErrorKind::Other, format!("waitpid failed: {}", Error::last_os_error())))
    }
    Ok(pid as usize)
}

#[cfg(target_os = "redox")]
fn wait(status: &mut i32) -> io::Result<usize> {
    libredox::call::waitpid(0, status, libc::WNOHANG).map_err(|e| {
        io::Error::new(ErrorKind::Other, format!("Error in waitpid(): {}", e.to_string()))
    })
}

fn bar_main(width: u32, height: u32) -> io::Result<()> {
    let mut bar = Bar::new(width, height);

    match Command::new("nexus-background").spawn() {
        Ok(child) => bar.children.push(("nexus-background".to_string(), child)),
        Err(err) => error!("failed to launch nexus-background: {}", err),
    }

    user_data! {
        enum Event { Time, Window }
    }
    let event_queue = EventQueue::<Event>::new().expect("launcher: failed to create event queue");

    let mut time_file = File::open(&format!("/scheme/time/{}", flag::CLOCK_MONOTONIC))?;

    event_queue.subscribe(time_file.as_raw_fd() as usize, Event::Time, event::EventFlags::READ)?;
    event_queue.subscribe(bar.window.as_raw_fd() as usize, Event::Window, event::EventFlags::READ)?;

    let mut mouse_x = -1;
    let mut mouse_y = -1;
    let mut mouse_left = false;
    let mut last_mouse_left = false;

    let all_events = [Event::Time, Event::Window].into_iter();

    'events: for event in all_events
        .chain(event_queue.map(|e| e.expect("launcher: failed to get next event").user_data))
    {
        match event {
            Event::Time => {
                let mut time_buf = [0_u8; core::mem::size_of::<TimeSpec>()];
                if time_file.read(&mut time_buf)? < mem::size_of::<TimeSpec>() { continue; }

                // Reap exited children
                let mut i = 0;
                while i < bar.children.len() {
                    let remove = match bar.children[i].1.try_wait() {
                        Ok(None) => false,
                        Ok(Some(status)) => {
                            info!("{} ({}) exited with {}", bar.children[i].0, bar.children[i].1.id(), status);
                            true
                        }
                        Err(err) => {
                            error!("failed to wait for {} ({}): {}", bar.children[i].0, bar.children[i].1.id(), err);
                            true
                        }
                    };
                    if remove { bar.children.remove(i); } else { i += 1; }
                }

                loop {
                    let mut status = 0;
                    let pid = wait(&mut status)?;
                    if pid == 0 { break; }
                }

                bar.update_time();
                bar.draw();

                match libredox::data::timespec_from_mut_bytes(&mut time_buf) {
                    time => {
                        time.tv_sec += 1;
                        time.tv_nsec = 0;
                    }
                }
                time_file.write(&time_buf)?;
            }
            Event::Window => {
                for event in bar.window.events() {
                    // Handle super key combos (unchanged)
                    if event.code >= 0x1000_0000 {
                        let mut super_event = event;
                        super_event.code -= 0x1000_0000;
                        match super_event.to_option() {
                            EventOption::Key(key_event) => match key_event.scancode {
                                orbclient::K_B if key_event.pressed => { bar.spawn("netsurf-fb".to_string()); }
                                orbclient::K_F if key_event.pressed => { bar.spawn("cosmic-files".to_string()); }
                                orbclient::K_T if key_event.pressed => { bar.spawn("cosmic-term".to_string()); }
                                _ => (),
                            },
                            _ => (),
                        }
                        continue;
                    }

                    let redraw = match event.to_option() {
                        EventOption::Mouse(mouse_event) => { mouse_x = mouse_event.x; mouse_y = mouse_event.y; true }
                        EventOption::Button(button_event) => {
                            if button_event.left { bar.suppress_start_open = false; }
                            mouse_left = button_event.left;
                            true
                        }
                        EventOption::Screen(screen_event) => {
                            bar.width = screen_event.width;
                            bar.height = screen_event.height;
                            bar.window.set_pos(0, screen_event.height as i32 - icon_size());
                            bar.window.set_size(screen_event.width, icon_size() as u32);
                            bar.selected = -2; // force redraw
                            bar.selected_window.set_pos(0, screen_event.height as i32);
                            bar.selected_window.set_size(screen_event.width, (font_size() + 8) as u32);
                            true
                        }
                        EventOption::Hover(hover_event) => {
                            if hover_event.entered { false } else { mouse_x = -1; mouse_y = -1; true }
                        }
                        EventOption::Quit(_) => break 'events,
                        _ => false,
                    };

                    if redraw {
                        let mut now_selected = -1;
                        {
                            let slot  = bar.window.height() as i32;
                            let count = 1 + bar.packages.len() as i32;
                            let total = count * slot;
                            let mut x = (bar.width as i32 - total) / 2;
                            let y = 0;
                            let mut i = 0;

                            // Start slot
                            if mouse_y >= y && mouse_x >= x && mouse_x < x + slot { now_selected = i; }
                            x += slot; i += 1;

                            // App slots
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

                        if !mouse_left && last_mouse_left {
                            let mut i = 0;

                            // Start button slot
                            if i == bar.selected {
                                if bar.start_menu_open {
                                    bar.suppress_start_open = true;
                                } else if !bar.suppress_start_open {
                                    bar.start_menu_open = true;

                                    match crate::config::mode() {
                                        crate::config::Mode::Desktop => {
                                            match crate::modes::desktop::show_desktop_menu(bar.width, bar.height, &mut bar.packages) {
                                                crate::modes::desktop::DesktopMenuResult::Launch(exec) => {
                                                    if !exec.trim().is_empty() { bar.spawn(exec); }
                                                }
                                                crate::modes::desktop::DesktopMenuResult::Logout => {
                                                    break 'events;
                                                }
                                                _ => {}
                                            }
                                        }
                                        crate::config::Mode::Mobile => {
                                            match crate::modes::mobile::show_mobile_menu(bar.width, bar.height, &mut bar.packages) {
                                                crate::modes::mobile::MobileMenuResult::Launch(exec) => {
                                                    if !exec.trim().is_empty() { bar.spawn(exec); }
                                                }
                                                crate::modes::mobile::MobileMenuResult::Logout => {
                                                    break 'events;
                                                }
                                                _ => {}
                                            }
                                        }
                                    }

                                    bar.start_menu_open = false;
                                    bar.suppress_start_open = true;
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

    debug!("Launcher exiting, killing {} children", bar.children.len());
    for (exec, child) in bar.children.iter_mut() {
        let pid = child.id();
        match child.kill() {
            Ok(()) => debug!("Successfully killed child: {}", pid),
            Err(err) => error!("failed to kill {} ({}): {}", exec, pid, err),
        }
        match child.wait() {
            Ok(status) => info!("{} ({}) exited with {}", exec, pid, status),
            Err(err) => error!("failed to wait for {} ({}): {}", exec, pid, err),
        }
    }

    // Reap leftover zombies
    debug!("Launcher exiting, reaping all zombie processes");
    let mut status = 0;
    while wait(&mut status).is_ok() {}

    Ok(())
}

fn chooser_main(paths: env::Args) {
    for ref path in paths.skip(1) {
        let mut packages = get_packages();

        packages.retain(|package| -> bool {
            for accept in package.accepts.iter() {
                if (accept.starts_with('*') && path.ends_with(&accept[1..]))
                    || (accept.ends_with('*') && path.starts_with(&accept[..accept.len() - 1]))
                {
                    return true;
                }
            }
            false
        });

        if packages.len() > 1 {
            let mut window = Window::new(
                -1,
                -1,
                chooser_width(),
                packages.len() as u32 * icon_small_size() as u32,
                path,
            )
            .expect("launcher: failed to open window");
            let font = Font::find(Some("Sans"), None, None).expect("launcher: failed to open font");

            let mut selected = -1;
            let mut mouse_y = 0;
            let mut mouse_left = false;
            let mut last_mouse_left = false;

            draw_chooser(&mut window, &font, &mut packages, selected);
            'choosing: loop {
                for event in window.events() {
                    let redraw = match event.to_option() {
                        EventOption::Mouse(mouse_event) => { mouse_y = mouse_event.y; true }
                        EventOption::Button(button_event) => { mouse_left = button_event.left; true }
                        EventOption::Quit(_) => break 'choosing,
                        _ => false,
                    };

                    if redraw {
                        let mut now_selected = -1;

                        let mut y = 0;
                        for (i, _package) in packages.iter().enumerate() {
                            if mouse_y >= y && mouse_y < y + icon_size() {
                                now_selected = i as i32;
                            }
                            y += icon_small_size();
                        }

                        if now_selected != selected {
                            selected = now_selected;
                            draw_chooser(&mut window, &font, &mut packages, selected);
                        }

                        if !mouse_left && last_mouse_left {
                            let mut y = 0;
                            for package in packages.iter() {
                                if mouse_y >= y && mouse_y < y + icon_small_size() {
                                    spawn_exec(&package.exec, Some(&path));
                                    break 'choosing;
                                }
                                y += icon_small_size();
                            }
                        }

                        last_mouse_left = mouse_left;
                    }
                }
            }
        } else if let Some(package) = packages.get(0) {
            spawn_exec(&package.exec, Some(&path));
        } else {
            error!("no application found for '{}'", path);
        }
    }
}

fn start_logging() {
    if let Err(e) = RedoxLogger::new()
        .with_output(
            OutputBuilder::stdout()
                .with_filter(log::LevelFilter::Debug)
                .with_ansi_escape_codes()
                .build(),
        )
        .with_process_name("launcher".into())
        .enable()
    {
        eprintln!("Launcher could not start logging: {}", e);
    }
}

fn main() -> Result<(), String> {
    start_logging();

    let (width, height) = orbclient::get_display_size()?;
    SCALE.store((height as isize / 1600) + 1, Ordering::Relaxed);
    let paths = env::args();
    if paths.len() > 1 {
        chooser_main(paths);
    } else {
        bar_main(width, height).map_err(|e| e.to_string())?;
    }

    Ok(())
}
