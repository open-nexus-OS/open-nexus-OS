// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The manifest's `restorable = true` (RFC-0100 §6, ADR-0086) — the app asks for its own
//! durable record: app-host writes its `@persist` fields to statefsd when the window is hidden
//! and when it closes, and restores them before the first frame of the next launch, so a kill
//! under memory pressure (memd) is never seen. The flag becomes the one grant
//! `nexus.permission.STATE` when bundlemgrd publishes the app's grants to policyd; the grant may
//! not be named in `caps` directly — the manifest states the need, the publisher derives the grant.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 1 test (the type and the direct-name refusal)

/// The grant the publisher derives — never written by hand.
const DERIVED_GRANT: &str = "nexus.permission.STATE";

/// Reads `restorable` (absent = false); rejects a non-boolean value and a `caps` entry that names
/// the derived grant directly.
pub(crate) fn validate(
    restorable: Option<&toml::Value>,
    capabilities: &[String],
) -> Result<bool, Box<dyn std::error::Error>> {
    let restorable = match restorable {
        None => false,
        Some(toml::Value::Boolean(b)) => *b,
        Some(_) => return Err("manifest.toml `restorable` must be `true` or `false`".into()),
    };
    if capabilities.iter().any(|c| c == DERIVED_GRANT) {
        return Err(format!(
            "manifest.toml `caps` may not name `{DERIVED_GRANT}` directly — \
             declare `restorable = true` instead"
        )
        .into());
    }
    Ok(restorable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reject_restorable_of_wrong_type_or_named_directly() {
        let caps = vec!["nexus.permission.WINDOW".to_string()];
        assert_eq!(validate(None, &caps).unwrap(), false);
        assert_eq!(validate(Some(&toml::Value::Boolean(true)), &caps).unwrap(), true);
        assert!(validate(Some(&toml::Value::String("yes".into())), &caps).is_err(), "type");
        assert!(validate(None, &["nexus.permission.STATE".to_string()]).is_err(), "named directly");
    }
}
