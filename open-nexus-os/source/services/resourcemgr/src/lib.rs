pub fn help() -> &'static str {
    "resourcemgr allocates localized assets. Usage: resourcemgr [--help]"
}

pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        help().to_string()
    } else {
        "resource manager initialized".to_string()
    }
}

pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::execute;

    #[test]
    fn default_message() {
        assert!(execute(&[]).contains("initialized"));
    }
}
