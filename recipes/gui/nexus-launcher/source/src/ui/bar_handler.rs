// src/ui/bar_handler.rs
// Bottom taskbar logic - extracted from main.rs for modularity

use std::io::{self, ErrorKind, Read, Write};
use std::fs::File;
use std::os::unix::io::AsRawFd;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use std::{env, mem};

use crate::utils::dpi_helper::{icon_size_legacy, font_size_legacy};
use crate::utils::dpi_helper as dpi_helper;
use crate::services::package_manager::{get_packages, load_png, UI_PATH};
use crate::services::process_manager::exec_to_command;
use crate::ui::chooser_handler::{chooser_width, icon_small_size, draw_chooser};
use libnexus::{THEME, IconVariant};
use libredox::flag;

use orbclient::{Color, EventOption, Renderer, Window, WindowFlag, K_ESC};
use orbclient::{ScreenEvent, ButtonEvent};
use orbimage::Image;
use orbfont::Font;
use libredox::call::waitpid;
use libredox::data::TimeSpec;
use libc;

use nexus_actionbar::{ActionBar, ActionBarMsg, Config as ActionBarConfig};
use libnexus::RedoxAnimationTimer;
use event::{user_data, EventQueue, Event, EventFlags};

use crate::dpi_scale;
use crate::config::settings::{Mode, mode, set_top_inset, BAR_HEIGHT, ICON_SCALE, ICON_SMALL_SCALE};
use crate::config::colors::{bar_paint, bar_highlight_paint, bar_activity_marker_paint};
use crate::config::colors::{text_paint, text_highlight_paint, text_inverse_fg, text_fg};
use crate::config::colors::load_crisp_font;
use crate::ui::layout::{SearchState, GridLayout, compute_grid, grid_iter_and_hit};
use crate::ui::components::draw_app_cell;
use crate::ui::icons::CommonIcons;
use crate::services::package_service::{IconSource, Package};
use crate::services::process_manager::{wait, reap_all_zombies};
use crate::types::state::{SCALE, LauncherState, WindowState};
use crate::modes::desktop::{show_desktop_menu, DesktopMenuResult};
use crate::modes::mobile::{show_mobile_menu, MobileMenuResult};

use log::{debug, error, info};

use std::collections::BTreeMap;

/// Bar structure - extracted from main.rs
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

        // Start icon on the bar (theme-managed, fallback PNG)
        let start = THEME
            .load_icon_sized("system/start", IconVariant::Auto, None)
            .unwrap_or_else(|| load_png(format!("{}/icons/places/start-here.png", UI_PATH), icon_size_legacy() as u32));

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
                (font_size_legacy() + 8.0) as u32,
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

        // Start icon slot (index 0)
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

        // App slots
        let dpi = dpi_scale();
        let target_icon = icon_size_legacy() as u32;
        for package in self.packages.iter_mut() {
            if i == self.selected {
                self.window.rect(x, 0, slot as u32, bar_h_u, bar_highlight_paint().color);

                self.selected_window.set(orbclient::Color::rgba(0, 0, 0, 0));
                let text = self.font.render(&package.name, dpi_helper::font_size(font_size_legacy()).round());
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
        let text = self.font.render(&self.time, dpi_helper::font_size(font_size_legacy() * 2.0).round());
        let tx = self.width as i32 - text.width() as i32 - 8;
        let ty = (bar_h_i - text.height() as i32) / 2;
        text.draw(&mut self.window, tx, ty, text_highlight_paint().color);

        self.window.sync();
    }

    /// Legacy small category chooser kept for nested categories
    fn start_window(&mut self, category_opt: Option<&String>) -> Option<String> {
        use orbclient::{EventOption, Window, WindowFlag, K_ESC};

        // Delegate to new desktop/mobile menus when no category is given
        if category_opt.is_none() {
            match mode() {
                Mode::Desktop => {
                    match show_desktop_menu(self.width, self.height, &mut self.packages) {
                        DesktopMenuResult::Launch(exec) => return Some(exec),
                        _ => return None,
                    }
                }
                Mode::Mobile => {
                    match show_mobile_menu(self.width, self.height, &mut self.packages) {
                        MobileMenuResult::Launch(exec) => return Some(exec),
                        _ => return None,
                    }
                }
            }
        }

        // Small category chooser (legacy)
        let packages = match category_opt {
            Some(category) => self.category_packages.get_mut(category)?,
            None => &mut self.start_packages,
        };

        let dpi = dpi_scale();
        let target = icon_small_size() as u32;

        let start_h = packages.len() as u32 * target;
        let mut start_window = Window::new_flags(
            0,
            self.height as i32 - icon_size_legacy() - start_h as i32,
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
                let text = self.font.render(&p.name, dpi_helper::font_size(font_size_legacy()).round());
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

/// Main bar function - extracted from main.rs for modularity
pub fn bar_main(width: u32, height: u32) -> io::Result<()> {
    let mut bar = Bar::new(width, height);

    // --- ActionBar: create top bar window + overlay panels window ---
    let dpi = dpi_scale();
    let mut actionbar = ActionBar::new(ActionBarConfig::default());
    let insets = actionbar.required_insets(width, height, dpi);

    // --- RedoxAnimationTimer: 30fps animation system ---
    let mut animation_timer = RedoxAnimationTimer::new();
    debug!("Starting RedoxAnimationTimer...");

    // Setze Callback für Animation-Updates
    animation_timer.set_callback(|| {
        debug!("RedoxAnimationTimer callback - Animation frame update");
        // Animation-Updates werden in Event::Time verarbeitet
        // Hier nur Debug-Ausgabe
    });

    animation_timer.start(); // Start 30fps timer
    debug!("RedoxAnimationTimer started successfully");

    // Publish the top inset globally so menus can respect it
    set_top_inset(insets.top);

    // Z-Buffer-System für Window-Hierarchie (Orbital-ähnlich)
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum WindowZOrder {
        Back = 0,
        Normal = 1,
        Front = 2,
        AlwaysOnTop = 3,
    }

    // Z-Buffer für Window-Management
    let mut zbuffer: Vec<(usize, WindowZOrder, usize)> = Vec::new();
    let mut windows: std::collections::BTreeMap<usize, Window> = std::collections::BTreeMap::new();
    let mut next_window_id = 1usize;

    // Top action bar window (always at top)
    let actionbar_id = next_window_id;
    next_window_id += 1;
    let mut actionbar_win = Window::new_flags(
        0, 0, width, insets.top,
        "NexusActionBar",
        &[WindowFlag::Async, WindowFlag::Borderless],
    ).expect("actionbar: failed to open window");

    // ActionBar in Z-Buffer einfügen (höchste Priorität)
    zbuffer.push((actionbar_id, WindowZOrder::AlwaysOnTop, 0));
    windows.insert(actionbar_id, actionbar_win);

    // Panels overlay window (drawn above normal windows)
    let panels_id = next_window_id;
    next_window_id += 1;
    let mut panels_win = Window::new_flags(
        0, 0, width, height,
        "NexusPanels",
        &[WindowFlag::Async, WindowFlag::Borderless, WindowFlag::Transparent],
    ).expect("actionbar panels: failed to open window");

    // Keep the panels window off-screen & tiny until needed
    panels_win.set_pos(-10_000, -10_000);
    panels_win.set_size(1, 1);

    // Panels in Z-Buffer einfügen (höchste Priorität nach ActionBar)
    zbuffer.push((panels_id, WindowZOrder::AlwaysOnTop, 1));
    windows.insert(panels_id, panels_win);

    // Wallpaper/background
    match Command::new("nexus-background").spawn() {
        Ok(child) => bar.children.push(("nexus-background".to_string(), child)),
        Err(err) => error!("failed to launch nexus-background: {}", err),
    }

    // Initial rendering of ActionBar so it appears immediately
    if let Some(actionbar_window) = windows.get_mut(&actionbar_id) {
        actionbar_window.set(orbclient::Color::rgba(0, 0, 0, 0));
        actionbar.render_bar(actionbar_window, 0, bar.width);
        actionbar_window.sync();
    }

    // Panels overlay visibility state (with short fade-out hold)
    const PANEL_FADEOUT_HOLD_MS: u64 = 240;
    let mut panels_visible = false;
    let mut panels_fadeout_deadline: Option<Instant> = None;

    // Event::Panels wird komplett blockiert - keine Rate-Limiting nötig

    user_data! {
        enum Event { Time, Bar, ActBar, Panels }
    }
    let event_queue = EventQueue::<Event>::new().expect("launcher: failed to create event queue");

    let mut event_count = 0u32;

    // Monotonic timer (adjust path if needed)
    let mut time_file = File::open("/scheme/time/4")?;

    // Initialize timer with current time + 1 second
    let mut time_buf = [0_u8; core::mem::size_of::<TimeSpec>()];
    match libredox::data::timespec_from_mut_bytes(&mut time_buf) {
        time => {
            time.tv_sec += 1;
            time.tv_nsec = 0;
        }
    }
    time_file.write(&time_buf)?;

    event_queue.subscribe(time_file.as_raw_fd() as usize, Event::Time,   event::EventFlags::READ)?;
    event_queue.subscribe(bar.window.as_raw_fd()      as usize, Event::Bar,     event::EventFlags::READ)?;

    // ActionBar und Panels aus Z-Buffer-System abrufen
    let actionbar_fd = windows.get(&actionbar_id).unwrap().as_raw_fd();
    let panels_fd = windows.get(&panels_id).unwrap().as_raw_fd();

    event_queue.subscribe(actionbar_fd as usize, Event::ActBar,  event::EventFlags::READ)?;
    event_queue.subscribe(panels_fd as usize, Event::Panels,  event::EventFlags::READ)?;

    debug!("Event-Subscription: Time={}, Bar={}, ActBar={}, Panels={}",
           time_file.as_raw_fd(), bar.window.as_raw_fd(), actionbar_fd, panels_fd);

    let mut mouse_x = -1;
    let mut mouse_y = -1;
    let mut mouse_left = false;
    let mut last_mouse_left = false;

    // Z-Buffer sortieren (höchste Priorität zuerst)
    zbuffer.sort_by(|a, b| b.1.cmp(&a.1));

    // Hit-Testing-Funktion für Mouse-Events (Orbital-ähnlich)
    // Wird direkt in Event-Handlern verwendet, um Borrow-Checker-Probleme zu vermeiden

    // UNIFIED EVENT-LOOP-ARCHITEKTUR
    // Einheitliches Event-System für alle Komponenten
    // Event::Panels wird komplett blockiert (verursacht 1000+ Events/Sekunde)

    'events: for ev in event_queue.map(|e| e.expect("launcher: failed to get next event")) {
        event_count += 1;
        debug!("Unified-Event-Loop: Processing event: {:?} (count: {})", ev.user_data, event_count);

        // Process events by type
        match ev.user_data {
            Event::Time => {
                // Handle timer events
                debug!("Event::Time - Timer event received!");
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

                // Update bottom bar clock & repaint
                bar.update_time();
                bar.draw();

                // Update ActionBar (Animation und State-Validation)
                actionbar.update(0);

                // Render ActionBar
                if let Some(actionbar_window) = windows.get_mut(&actionbar_id) {
                    actionbar_window.set(orbclient::Color::rgba(0, 0, 0, 0));
                    actionbar.render_bar(actionbar_window, 0, bar.width);
                    actionbar_window.sync();
                }

                // Panels overlay visibility with fade-out grace
                let now = Instant::now();
                let any_animation_running = actionbar.is_animating();
                let want_visible = if actionbar.any_panel_open() || any_animation_running {
                    panels_fadeout_deadline = None;
                    true
                } else {
                    if panels_fadeout_deadline.is_none() {
                        panels_fadeout_deadline = Some(now + Duration::from_millis(PANEL_FADEOUT_HOLD_MS));
                    }
                    panels_fadeout_deadline.unwrap() > now
                };

                if want_visible && !panels_visible {
                    if let Some(panels_window) = windows.get_mut(&panels_id) {
                        panels_window.set_pos(0, 0);
                        panels_window.set_size(bar.width, bar.height);
                    }
                    panels_visible = true;
                } else if !want_visible && panels_visible {
                    if let Some(panels_window) = windows.get_mut(&panels_id) {
                        panels_window.set_pos(-10_000, -10_000);
                        panels_window.set_size(1, 1);
                    }
                    panels_visible = false;
                }

                // Render overlay panels (if visible)
                if let Some(panels_window) = windows.get_mut(&panels_id) {
                    panels_window.set(orbclient::Color::rgba(0, 0, 0, 0));
                    actionbar.render_panels(panels_window, bar.width, bar.height);
                    panels_window.sync();
                }

                // Re-arm timer
                match libredox::data::timespec_from_mut_bytes(&mut time_buf) {
                    time => {
                        time.tv_sec += 1;
                        time.tv_nsec = 0;
                    }
                }
                time_file.write(&time_buf)?;
            }
            Event::ActBar => {
                // ActionBar-Events über Z-Buffer-System verarbeiten
                debug!("Event::ActBar - Processing ActionBar events via Z-Buffer");
                if let Some(actionbar_window) = windows.get_mut(&actionbar_id) {
                    let mut event_count = 0;
                    for ev_win in actionbar_window.events() {
                        event_count += 1;
                        debug!("ActionBar window event #{}: {:?}", event_count, ev_win);

                        // Mouse-Events über Hit-Testing verarbeiten
                        let ev_option = ev_win.to_option();
                        match ev_option {
                            orbclient::EventOption::Mouse(mouse_ev) => {
                                mouse_x = mouse_ev.x;
                                mouse_y = mouse_ev.y;
                                debug!("Mouse event at ({}, {})", mouse_x, mouse_y);
                                // Hit-Testing direkt hier implementieren
                                debug!("Hit-Testing: Mouse at ({}, {})", mouse_x, mouse_y);

                                // Prüfe ob Mouse innerhalb des ActionBar-Windows ist
                                if mouse_x >= 0 && mouse_y >= 0 &&
                                   mouse_x < actionbar_window.width() as i32 && mouse_y < actionbar_window.height() as i32 {
                                    debug!("Hit-Testing: ActionBar hit at ({}, {})", mouse_x, mouse_y);
                                    // ActionBar-Event verarbeiten
                                }
                            }
                            orbclient::EventOption::Button(button_ev) => {
                                mouse_left = button_ev.left;
                                if button_ev.left && !last_mouse_left {
                                    debug!("Mouse click at ({}, {})", mouse_x, mouse_y);
                                    // Hit-Testing direkt hier implementieren
                                    debug!("Hit-Testing: Mouse click at ({}, {})", mouse_x, mouse_y);

                                    // Prüfe ob Mouse innerhalb des ActionBar-Windows ist
                                    if mouse_x >= 0 && mouse_y >= 0 &&
                                       mouse_x < actionbar_window.width() as i32 && mouse_y < actionbar_window.height() as i32 {
                                        debug!("Hit-Testing: ActionBar hit - processing click event");
                                        // ActionBar-Event verarbeiten
                                        if let Some(msg) = actionbar.handle_event(&ev_win) {
                                            debug!("ActionBar message: {:?}", msg);
                                            match msg {
                                                ActionBarMsg::DismissPanels => { /* handled by visibility policy */ }
                                                ActionBarMsg::RequestInsetUpdate(new_insets) => {
                                                    actionbar_window.set_size(bar.width, new_insets.top);
                                                    set_top_inset(new_insets.top);
                                                }
                                            }
                                        }
                                    }
                                }
                                last_mouse_left = button_ev.left;
                            }
                            _ => {
                                // Andere Events direkt an ActionBar weiterleiten
                                if let Some(msg) = actionbar.handle_event(&ev_win) {
                                    debug!("ActionBar message: {:?}", msg);
                                    match msg {
                                        ActionBarMsg::DismissPanels => { /* handled by visibility policy */ }
                                        ActionBarMsg::RequestInsetUpdate(new_insets) => {
                                            actionbar_window.set_size(bar.width, new_insets.top);
                                            set_top_inset(new_insets.top);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if event_count == 0 {
                        debug!("ActionBar window: NO EVENTS received");
                    }
                }
            }
            Event::Bar => {
                // Handle bar events
                debug!("Event::Bar - Processing bar events");
                for event in bar.window.events() {
                    // Handle Super+key combos
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
                            // Resize bottom bar windows
                            bar.width = screen_event.width;
                            bar.height = screen_event.height;
                            bar.window.set_pos(0, screen_event.height as i32 - icon_size_legacy());
                            bar.window.set_size(screen_event.width, icon_size_legacy() as u32);
                            bar.selected = -2; // force redraw
                            bar.selected_window.set_pos(0, screen_event.height as i32);
                            bar.selected_window.set_size(screen_event.width, (font_size_legacy() + 8.0) as u32);

                            // Keep ActionBar & panels in sync, and update global top inset
                            let dpi = dpi_scale();
                            let insets = actionbar.required_insets(screen_event.width, screen_event.height, dpi);
                            set_top_inset(insets.top);

                            if let Some(w) = windows.get_mut(&actionbar_id) {
                                w.set_pos(0, 0);
                                w.set_size(screen_event.width, insets.top);
                            }

                            // Apply current overlay visibility policy on resize
                            let now = Instant::now();
                            let want_visible = if actionbar.any_panel_open() {
                                panels_fadeout_deadline = None;
                                true
                            } else {
                                if panels_fadeout_deadline.is_none() {
                                    panels_fadeout_deadline = Some(now + Duration::from_millis(PANEL_FADEOUT_HOLD_MS));
                                }
                                panels_fadeout_deadline.unwrap() > now
                            };

                            if want_visible && !panels_visible {
                                if let Some(w) = windows.get_mut(&panels_id) {
                                    w.set_pos(0, 0);
                                    w.set_size(screen_event.width, screen_event.height);
                                }
                                panels_visible = true;
                            } else if !want_visible && panels_visible {
                                if let Some(w) = windows.get_mut(&panels_id) {
                                    w.set_pos(-10_000, -10_000);
                                    w.set_size(1, 1);
                                }
                                panels_visible = false;
                            }
                            true
                        }
                        EventOption::Hover(hover_event) => {
                            if hover_event.entered { false } else { mouse_x = -1; mouse_y = -1; true }
                        }
                        EventOption::Quit(_) => break 'events,
                        _ => false,
                    };

                    if redraw {
                        // Hit-testing for slots
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

                        // Mouse button release logic
                        if !mouse_left && last_mouse_left {
                            let mut i = 0;

                            // Start button slot
                            if i == bar.selected {
                                if bar.start_menu_open {
                                    bar.suppress_start_open = true;
                                } else if !bar.suppress_start_open {
                                    bar.start_menu_open = true;

                                    // Ensure panels overlay is out of the way while Start menu is open
                                    panels_fadeout_deadline = None;
                                    if panels_visible {
                                        if let Some(panels_window) = windows.get_mut(&panels_id) {
                                            panels_window.set_pos(-10_000, -10_000);
                                            panels_window.set_size(1, 1);
                                        }
                                        panels_visible = false;
                                    }

                                    match mode() {
                                        Mode::Desktop => {
                                            match show_desktop_menu(bar.width, bar.height, &mut bar.packages) {
                                                DesktopMenuResult::Launch(exec) => {
                                                    if !exec.trim().is_empty() { bar.spawn(exec); }
                                                }
                                                DesktopMenuResult::Logout => {
                                                    break 'events;
                                                }
                                                _ => {}
                                            }
                                        }
                                        Mode::Mobile => {
                                            match show_mobile_menu(bar.width, bar.height, &mut bar.packages) {
                                                 MobileMenuResult::Launch(exec) => {
                                                    if !exec.trim().is_empty() { bar.spawn(exec); }
                                                }
                                                 MobileMenuResult::Logout => {
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
            Event::Panels => {
                // Block Event::Panels completely to prevent spam
                debug!("Event::Panels - BLOCKED! Ignoring completely");
                continue;
            }
        }

        // Simple event counter for debugging
        if event_count % 100 == 0 {
            debug!("Processed {} events", event_count);
        }
    }

    // Stop animation timer to prevent resource leak
    debug!("Stopping RedoxAnimationTimer...");
    animation_timer.stop();

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
