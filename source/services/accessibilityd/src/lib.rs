// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Accessibility daemon domain library – service API and CLI handlers
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test (provides_hint)
//! ADR: docs/adr/0017-service-architecture.md
pub fn help() -> &'static str {
    "accessibilityd surfaces assistive metadata. Usage: accessibilityd [--help] hint"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        return help().to_string();
    }
    if let Some(hint) = args.first() {
        return format!("accessibility hint: {hint}");
    }
    "accessibilityd awaiting hint".to_string()
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
    fn provides_hint() {
        assert!(execute(&["focus"]).contains("focus"));
    }
}
