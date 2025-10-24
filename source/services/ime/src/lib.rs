//! CONTEXT: Input Method Engine daemon domain library (service API and handlers)
//! INTENT: Input method (keyboard), compose/commit, candidate UI
//! IDL (target): attachClient(win), setComposingText(text), commitText(text), show/hide()
//! DEPS: systemui, compositor, accessibilityd
//! READINESS: print "ime: ready"; register/heartbeat with samgr
//! TESTS: attachClient mock; transform uppercase
pub fn help() -> &'static str {
    "ime provides input methods. Usage: ime [--help] text"
}

pub fn transform(input: &str) -> String {
    input.to_uppercase()
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        return help().to_string();
    }
    if let Some(text) = args.first() {
        return transform(text);
    }
    "ime awaiting text".to_string()
}

pub fn run() {
    let owned: Vec<String> = std::env::args().skip(1).collect();
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    println!("{}", execute(&refs));
}

#[cfg(test)]
mod tests {
    use super::{execute, transform};

    #[test]
    fn uppercase_conversion() {
        assert_eq!(transform("abc"), "ABC");
        assert_eq!(execute(&["abc"]), "ABC");
    }
}
