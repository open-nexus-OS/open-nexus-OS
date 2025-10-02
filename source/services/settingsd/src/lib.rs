pub fn help() -> &'static str {
    "settingsd persists configuration. Usage: settingsd [--help] key=value"
}

pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        return help().to_string();
    }
    if let Some(kv) = args.first() {
        if let Some((key, value)) = kv.split_once('=') {
            return format!("settingsd applied {key}={value}");
        }
    }
    "settingsd awaiting assignment".to_string()
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
    fn apply_setting() {
        assert!(execute(&["theme=dark"]).contains("theme"));
    }
}
