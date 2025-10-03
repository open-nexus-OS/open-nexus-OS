//! Clipboard storage logic shared with the OS daemon.

#![forbid(unsafe_code)]

use std::sync::Mutex;

#[cfg(all(feature = "backend-host", feature = "backend-os"))]
compile_error!("Choose exactly one backend feature.");

#[cfg(not(any(feature = "backend-host", feature = "backend-os")))]
compile_error!("Select a backend feature.");

static CLIPBOARD: Mutex<Option<String>> = Mutex::new(None);

/// Returns the CLI usage string for clipboardd.
pub fn help() -> &'static str {
    "clipboard stores shared text. Usage: clipboard [--help] [--set value]"
}

/// Executes the clipboard CLI and returns a response string.
pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        return help().to_string();
    }
    if let Some(pos) = args.iter().position(|arg| *arg == "--set") {
        if let Some(value) = args.get(pos + 1) {
            if let Ok(mut guard) = CLIPBOARD.lock() {
                *guard = Some((*value).to_string());
            }
            return format!("clipboard updated to {value}");
        }
    }
    CLIPBOARD
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or_else(|| "clipboard empty".to_string())
}

/// Entry point used by the daemon to forward CLI arguments.
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{execute, CLIPBOARD};

    #[test]
    fn set_and_get() {
        let _ = CLIPBOARD.lock().map(|mut guard| *guard = None);
        execute(&["--set", "hello"]);
        assert!(execute(&[]).contains("hello"));
    }
}
