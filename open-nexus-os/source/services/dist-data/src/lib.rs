pub fn help() -> &'static str {
    "dist-data replicates state across devices. Usage: dist-data [--help] token"
}

pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        return help().to_string();
    }
    if let Some(token) = args.first() {
        let status = dsoftbus::execute(&["--status", token]);
        return format!("dist-data sync via {status}");
    }
    "dist-data awaiting token".to_string()
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
    fn sync_invokes_bus() {
        let msg = execute(&["node8"]);
        assert!(msg.contains("dsoftbus"));
    }
}
