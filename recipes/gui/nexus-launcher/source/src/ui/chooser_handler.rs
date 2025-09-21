// src/ui/chooser_handler.rs
// File chooser logic for opening files with appropriate applications

use std::env;
use orbclient::{EventOption, Renderer, Window, WindowFlag};
use orbfont::Font;
use crate::services::package_service::Package;
use crate::services::package_manager::{get_packages, filter_packages_for_path};
use crate::services::process_manager::spawn_exec;
use crate::config::colors::{bar_paint, bar_highlight_paint, text_paint, text_highlight_paint, load_crisp_font};
use crate::config::settings::{BAR_HEIGHT, ICON_SCALE, ICON_SMALL_SCALE};
use crate::utils::dpi_helper;

/// Get chooser window width based on scale
pub fn chooser_width() -> u32 {
    use crate::types::state::SCALE;
    use std::sync::atomic::Ordering;
    200 * SCALE.load(Ordering::Relaxed) as u32
}

/// Get icon size for small icons
pub fn icon_small_size() -> i32 {
    (icon_size() as f32 * ICON_SMALL_SCALE).round() as i32
}

/// Get base icon size
fn icon_size() -> i32 {
    (BAR_HEIGHT as f32 * ICON_SCALE).round() as i32
}

/// Get font size
fn font_size() -> i32 {
    (icon_size() as f32 * 0.5).round() as i32
}

/// Draw the chooser window with package list
pub fn draw_chooser(window: &mut Window, font: &Font, packages: &mut Vec<Package>, selected: i32) {
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

/// Main chooser function - shows application selection for file paths
pub fn chooser_main(paths: env::Args) {
    for ref path in paths.skip(1) {
        let all_packages = get_packages();
        let mut packages = filter_packages_for_path(&all_packages, path);

        if packages.len() > 1 {
            // Multiple applications can handle this file - show chooser
            let mut window = Window::new(
                -1,
                -1,
                chooser_width(),
                packages.len() as u32 * icon_small_size() as u32,
                path,
            )
            .expect("launcher: failed to open window");
            let font = load_crisp_font();

            let mut selected = -1;
            let mut mouse_y = 0;
            let mut mouse_left = false;
            let mut last_mouse_left = false;

            draw_chooser(&mut window, &font, &mut packages, selected);
            
            'choosing: loop {
                for event in window.events() {
                    let redraw = match event.to_option() {
                        EventOption::Mouse(mouse_event) => { 
                            mouse_y = mouse_event.y; 
                            true 
                        }
                        EventOption::Button(button_event) => { 
                            mouse_left = button_event.left; 
                            true 
                        }
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
            // Single application can handle this file - launch directly
            spawn_exec(&package.exec, Some(&path));
        } else {
            log::error!("no application found for '{}'", path);
        }
    }
}

/// Get DPI scale factor
fn dpi_scale() -> f32 {
    dpi_helper::get_dpi_scale()
}
