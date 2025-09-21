// src/main.rs
// Launcher entry point: minimal setup and routing to modular handlers

extern crate orbclient;
extern crate redox_log;

use redox_log::{OutputBuilder, RedoxLogger};
use std::env;
use std::sync::atomic::{AtomicIsize, Ordering};

// Import modular handlers
use nexus_launcher::ui::{chooser_handler, bar_handler};

// Global scale factor
static SCALE: AtomicIsize = AtomicIsize::new(1);

/// Setup logging system
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
        // Use modular chooser handler
        chooser_handler::chooser_main(paths);
    } else {
        // Use modular bar handler
        bar_handler::bar_main(width, height).map_err(|e| e.to_string())?;
    }

    Ok(())
}
