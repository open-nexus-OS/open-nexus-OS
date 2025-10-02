pub fn help() -> &'static str {
    "abilitymgr manages ability lifecycle. Usage: abilitymgr [--help]"
}

pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        help().to_string()
    } else {
        "ability manager ready".to_string()
    }
}

pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{execute, help};

    #[test]
    fn help_message() {
        assert!(help().contains("abilitymgr"));
    }

    #[test]
    fn execute_help() {
        let output = execute(&["--help"]);
        assert!(output.contains("Usage"));
    }
}
