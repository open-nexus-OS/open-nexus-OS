pub fn help() -> &'static str {
    "dsoftbus coordinates distributed channels. Usage: dsoftbus [--help] [--status token]"
}

pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        return help().to_string();
    }
    if let Some(pos) = args.iter().position(|arg| *arg == "--status") {
        if let Some(token) = args.get(pos + 1) {
            if identity::validate(token) {
                return format!("dsoftbus link healthy for {token}");
            }
            return "dsoftbus identity invalid".to_string();
        }
    }
    "dsoftbus ready".to_string()
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
    fn status_validates_identity() {
        let result = execute(&["--status", "node7"]);
        assert!(result.contains("healthy"));
    }
}
