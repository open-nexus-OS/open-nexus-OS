pub fn help() -> &'static str {
    "batterymgr tracks charge levels. Usage: batterymgr [--help]"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        help().to_string()
    } else {
        "battery manager reporting nominal".to_string()
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
    fn nominal_output() {
        assert!(execute(&[]).contains("nominal"));
    }
}
