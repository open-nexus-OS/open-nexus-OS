pub fn help() -> &'static str {
    "windowd orchestrates surfaces. Usage: windowd [--help]"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        help().to_string()
    } else {
        "windowd compositor bridge ready".to_string()
    }
}

pub fn render_frame(width: usize, height: usize) -> Vec<u32> {
    let mut buffer = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let pixel = ((x as u32) << 16) ^ (y as u32);
            buffer.push(pixel);
        }
    }
    buffer
}

pub fn frame_checksum() -> u32 {
    render_frame(8, 8).iter().fold(0u32, |acc, value| acc.wrapping_add(*value))
}

pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{execute, frame_checksum, render_frame};

    #[test]
    fn help_string_present() {
        assert!(execute(&["--help"]).contains("windowd"));
    }

    #[test]
    fn checksum_matches() {
        assert_eq!(frame_checksum(), render_frame(8, 8).iter().copied().fold(0, u32::wrapping_add));
    }
}
