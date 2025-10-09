pub fn help() -> &'static str {
    "logd collects structured records. Usage: logd [--help] message"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        return help().to_string();
    }
    if let Some(message) = args.first() {
        return format!("logd captured: {message}");
    }
    "logd awaiting input".to_string()
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
    fn echoes_message() {
        assert!(execute(&["hello"]).contains("hello"));
    }
}
