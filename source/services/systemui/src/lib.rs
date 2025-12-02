//! CONTEXT: System UI service domain library (service API and handlers)
//! INTENT: Statusbar, launcher, shell UI
//! IDL (target): showLauncher(), setStatus(icon,state), setTheme(theme)
//! DEPS: compositor/ime/notifd
//! READINESS: print "systemui: ready"; register/heartbeat with samgr
//! TESTS: show launcher mock; frame checksum stable
pub fn help() -> &'static str {
    "systemui draws system chrome. Usage: systemui [--help] [--boot-animation]"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        return help().to_string();
    }
    if args.contains(&"--boot-animation") {
        return "systemui playing boot animation".to_string();
    }
    "systemui ready".to_string()
}

pub fn compose_frame() -> Vec<u32> {
    let mut frame = compositor::compose();
    for (idx, pixel) in frame.iter_mut().enumerate() {
        if idx % 7 == 0 {
            *pixel = pixel.wrapping_add(0x00FF_00FF);
        }
    }
    frame
}

pub fn checksum() -> u32 {
    compose_frame()
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
    use super::{checksum, compose_frame, execute};

    #[test]
    fn help_flag() {
        assert!(execute(&["--help"]).contains("systemui"));
    }

    #[test]
    fn checksum_expected() {
        let expected = compose_frame().iter().copied().fold(0, u32::wrapping_add);
        assert_eq!(checksum(), expected);
    }
}
