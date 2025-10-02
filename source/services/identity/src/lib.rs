pub fn help() -> &'static str {
    "identity validates distributed principals. Usage: identity [--help]"
}

pub fn validate(token: &str) -> bool {
    token.len() >= 4 && token.chars().all(|c| c.is_ascii_alphanumeric())
}

pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        help().to_string()
    } else if let Some(token) = args.first() {
        if validate(token) {
            format!("identity accepted {token}")
        } else {
            "identity rejected token".to_string()
        }
    } else {
        "identity idle".to_string()
    }
}

pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{execute, validate};

    #[test]
    fn rejects_short_token() {
        assert!(!validate("ab"));
    }

    #[test]
    fn accepts_valid_token() {
        assert!(validate("node1"));
        assert!(execute(&["node1"]).contains("accepted"));
    }
}
