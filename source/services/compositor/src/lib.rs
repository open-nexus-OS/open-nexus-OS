//! CONTEXT: Compositor daemon domain library (service API and handlers)
//! INTENT: Surface/layer composition, VSync, window Z-order
//! IDL (target): createSurface(token), commit(surface,rects), setLayer(win,z), subscribeVsync()
//! DEPS: systemui, windowd (if separate)
//! READINESS: print "compositor: ready"; register/heartbeat with samgr
//! TESTS: VSync tick; frame checksum stable
pub fn help() -> &'static str {
    "compositor composites window surfaces. Usage: compositor [--help]"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        help().to_string()
    } else {
        "compositor ready".to_string()
    }
}

pub fn compose() -> Vec<u32> {
    let mut frame = windowd::render_frame(8, 8);
    for (index, pixel) in frame.iter_mut().enumerate() {
        let overlay = (index as u32) << 8;
        *pixel = pixel.wrapping_add(overlay);
    }
    frame
}

pub fn checksum() -> u32 {
    compose()
        .iter()
        .fold(0_u32, |acc, value| acc.wrapping_add(*value))
}

pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{checksum, compose, execute};

    #[test]
    fn help_path() {
        assert!(execute(&["--help"]).contains("compositor"));
    }

    #[test]
    fn checksum_matches_manual_sum() {
        let expected = compose().iter().copied().fold(0, u32::wrapping_add);
        assert_eq!(checksum(), expected);
    }
}
