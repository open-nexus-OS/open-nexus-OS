pub fn help() -> &'static str {
    "media orchestrates playback. Usage: media [--help] [--probe asset]"
}

pub fn execute(args: &[&str]) -> String {
    if args.iter().any(|arg| *arg == "--help") {
        return help().to_string();
    }
    if let Some(pos) = args.iter().position(|arg| *arg == "--probe") {
        if let Some(asset) = args.get(pos + 1) {
            return format!("media pipeline ready for {asset}");
        }
    }
    "media idle".to_string()
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
    fn probe_asset() {
        assert!(execute(&["--probe", "song.mp3"]).contains("song.mp3"));
    }
}
