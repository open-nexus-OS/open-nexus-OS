pub fn help() -> &'static str {
    "time-sync aligns clocks. Usage: time-sync [--help] offset"
}

pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        return help().to_string();
    }
    if let Some(offset) = args.first() {
        if let Ok(delta) = offset.parse::<i64>() {
            return format!("time sync applying offset {delta} ppm");
        }
    }
    "time-sync awaiting offset".to_string()
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
    fn parses_offset() {
        let msg = execute(&["-12"]);
        assert!(msg.contains("-12"));
    }
}
