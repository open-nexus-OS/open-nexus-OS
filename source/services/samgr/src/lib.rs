pub fn help() -> &'static str {
    "samgr orchestrates service abilities. Usage: samgr [--help]"
}

pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        help().to_string()
    } else {
        "service ability manager ready".to_string()
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
    fn help_contains_name() {
        assert!(help().contains("samgr"));
    }

    #[test]
    fn exec_default() {
        assert!(execute(&[]).contains("ready"));
    }
}
