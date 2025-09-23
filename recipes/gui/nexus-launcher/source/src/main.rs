// src/main.rs
// Launcher entry point: logging, DPI init, optional env overrides for mode/theme,
// then route to modular handlers.

extern crate orbclient;
extern crate redox_log;

use redox_log::{OutputBuilder, RedoxLogger};
use std::env;

use nexus_launcher::ui::{chooser_handler, bar_handler};
use nexus_launcher::utils::dpi_helper;
use nexus_launcher::config::settings::{self, Mode, ThemeMode};
use nexus_launcher::types::state::SCALE; // global scale used by chooser

/// Setup logging early to see init problems.
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

/// Optionally honor environment overrides for mode/theme at startup.
/// This is a simple bootstrap; runtime toggles should call settings::set_* APIs.
fn apply_env_overrides() {
    if let Ok(s) = env::var("NEXUS_MODE") {
        match s.to_ascii_lowercase().as_str() {
            "mobile"  => settings::set_mode(Mode::Mobile),
            "desktop" => settings::set_mode(Mode::Desktop),
            _ => {}
        }
    }
    if let Ok(s) = env::var("NEXUS_THEME") {
        match s.to_ascii_lowercase().as_str() {
            "dark"  => settings::set_theme_mode(ThemeMode::Dark),
            "light" => settings::set_theme_mode(ThemeMode::Light),
            _ => {}
        }
    }
}

fn main() -> Result<(), String> {
    start_logging();

    // Screen + DPI
    let (width, height) = orbclient::get_display_size()?;
    dpi_helper::init_dpi_scaling(width, height);

    // Keep existing SCALE semantics used by chooser: very coarse global factor.
    // (If you later move chooser to pure DPI, you can drop SCALE.)
    use std::sync::atomic::Ordering;
    SCALE.store((height as isize / 1600) + 1, Ordering::Relaxed);

    // Apply optional environment overrides (useful for testing)
    apply_env_overrides();

    // Decide between file-chooser and bar.
    // NOTE: We keep the legacy behavior: any extra CLI arguments are treated as file paths
    // for the chooser. If you add CLI flags later, parse them first and strip from args.
    let mut args = env::args();
    let _exe_name = args.next();
    if args.len() > 0 {
        // Reconstruct an iterator including program name + paths for chooser_main
        // chooser_main expects env::Args; easiest is to call it on env::args() directly.
        chooser_handler::chooser_main(env::args());
    } else {
        bar_handler::bar_main(width, height).map_err(|e| e.to_string())?;
    }

    Ok(())
}
