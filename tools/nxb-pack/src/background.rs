// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The manifest's `background = [...]` (RFC-0111 §6, ADR-0084) — the modes an app
//! claims to need while hidden. A non-empty list becomes the one grant
//! `nexus.permission.BACKGROUND` when bundlemgrd publishes the app's grants to policyd; the
//! list itself is bounded HERE, at pack time, the earliest point. Not role-gated: any app may
//! declare it (whether it is honored under pressure is the runtime's, 6e). The grant may not be
//! named in `caps` directly — the manifest states the need, the publisher derives the grant.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 test (the bounds and the direct-name refusal)

/// Most modes one manifest may list.
const MAX_MODES: usize = 8;
/// Longest mode name.
const MAX_MODE_BYTES: usize = 32;
/// The grant the publisher derives — never written by hand.
const DERIVED_GRANT: &str = "nexus.permission.BACKGROUND";

/// Rejects an over-long list, a malformed mode (`1..=32` of `[a-z0-9-]`) and a `caps` entry
/// that names the derived grant directly.
pub(crate) fn validate(
    background: &[String],
    capabilities: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    if background.len() > MAX_MODES {
        return Err(format!("manifest.toml `background` lists more than {MAX_MODES} modes").into());
    }
    for mode in background {
        let ok = !mode.is_empty()
            && mode.len() <= MAX_MODE_BYTES
            && mode.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !ok {
            return Err(format!(
                "manifest.toml `background` mode `{mode}` must be 1..={MAX_MODE_BYTES} of [a-z0-9-]"
            )
            .into());
        }
    }
    if capabilities.iter().any(|c| c == DERIVED_GRANT) {
        return Err(format!(
            "manifest.toml `caps` may not name `{DERIVED_GRANT}` directly — \
             declare `background = [\"<mode>\"]` instead"
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_reject_background_out_of_bounds_or_named_directly() {
        assert!(validate(&strs(&["audio", "sync-1"]), &strs(&["nexus.permission.WINDOW"])).is_ok());
        assert!(validate(&[], &[]).is_ok());
        assert!(validate(&strs(&["Audio"]), &[]).is_err(), "upper case");
        assert!(validate(&strs(&[""]), &[]).is_err(), "empty");
        assert!(validate(&strs(&["a".repeat(33).as_str()]), &[]).is_err(), "too long");
        assert!(validate(&strs(&["a"; 9]), &[]).is_err(), "too many");
        assert!(validate(&[], &strs(&["nexus.permission.BACKGROUND"])).is_err(), "named directly");
    }
}
