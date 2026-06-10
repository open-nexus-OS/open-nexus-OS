// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Thermal manager daemon domain library – service API and CLI handlers
//! OWNERS: @runtime
//! STATUS: Placeholder
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 unit test (stable_message)
//! ADR: docs/adr/0017-service-architecture.md
pub fn help() -> &'static str {
    "thermalmgr balances device thermals. Usage: thermalmgr [--help]"
}

pub fn execute(args: &[&str]) -> String {
    if args.contains(&"--help") {
        help().to_string()
    } else {
        "thermal manager stable".to_string()
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
    fn stable_message() {
        assert!(execute(&[]).contains("stable"));
    }
}
