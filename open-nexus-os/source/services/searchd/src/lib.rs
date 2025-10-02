pub fn help() -> &'static str {
    "searchd indexes local content. Usage: searchd [--help]"
}

pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        help().to_string()
    } else {
        "search daemon indexing".to_string()
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
    fn indexing_message() {
        assert!(execute(&[]).contains("indexing"));
    }
}
