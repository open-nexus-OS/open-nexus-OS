// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Media service domain library – service API and CLI handlers
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test (probe_asset)
//! ADR: docs/adr/0017-service-architecture.md
pub fn help() -> &'static str {
    "media orchestrates playback. Usage: media [--help] [--probe asset]"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
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
