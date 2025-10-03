//! Search daemon domain logic shared with the thin OS adapter.

#![forbid(unsafe_code)]

#[cfg(all(feature = "backend-host", feature = "backend-os"))]
compile_error!("Choose exactly one backend feature.");

#[cfg(not(any(feature = "backend-host", feature = "backend-os")))]
compile_error!("Select a backend feature.");

/// Returns the CLI usage string.
pub fn help() -> &'static str {
    "searchd indexes local content. Usage: searchd [--help]"
}

/// Executes the search CLI command.
pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        help().to_string()
    } else {
        "search daemon indexing".to_string()
    }
}

/// Entry point used by the daemon to process CLI arguments.
pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::execute;

    #[test]
    fn indexing_message() {
        assert!(execute(&[]).contains("indexing"));
    }
}
