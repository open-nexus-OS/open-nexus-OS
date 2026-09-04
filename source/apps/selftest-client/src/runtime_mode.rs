// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared runtime-mode/profile token parsing for proof vs interactive starts.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: Unit tests in this file plus startup contract tests
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeMode {
    Proof,
    InteractiveMinimal,
    InteractiveFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeProfile {
    Full,
    Bringup,
    Quick,
    Ota,
    Net,
    None,
    /// TASK-0050 reset lane: full ladder + the reset-proof trigger in
    /// bringup (sentinel discipline; the trigger is read RAW from fw_cfg —
    /// proof boots keep the full phase scope regardless).
    Reset,
    /// TASK-0179 crown lane: the two-boot OTA flip (stage the real os-B,
    /// switch, reset; boot 2 proves the loader chose the new slot and the
    /// quorum commits it). Same reduced phase scope as `Ota`.
    OtaFlip,
    /// TASK-0321 P3 bundle-set crown lane (two boots, one uart; bringup +
    /// end scope like `OtaFlip`).
    OtaBundle,
    /// TASK-0289-B loader backstop: the four-boot tries-exhaustion lane
    /// (stage + switch, two bricked trial boots — init parks —, loader
    /// exhaustion flip back to slot a, rollback observation). Reduced
    /// phase scope like `OtaFlip`.
    OtaFallback,
}

#[must_use]
pub(crate) fn parse_runtime_mode(bytes: &[u8]) -> Option<RuntimeMode> {
    match trim_ascii_token(bytes) {
        b"proof" => Some(RuntimeMode::Proof),
        b"interactive-minimal" => Some(RuntimeMode::InteractiveMinimal),
        b"interactive-full" => Some(RuntimeMode::InteractiveFull),
        _ => None,
    }
}

#[must_use]
pub(crate) fn parse_runtime_profile(bytes: &[u8]) -> Option<RuntimeProfile> {
    match trim_ascii_token(bytes) {
        b"full" => Some(RuntimeProfile::Full),
        b"bringup" => Some(RuntimeProfile::Bringup),
        b"quick" => Some(RuntimeProfile::Quick),
        b"ota" => Some(RuntimeProfile::Ota),
        b"ota-flip" => Some(RuntimeProfile::OtaFlip),
        b"ota-bundle" => Some(RuntimeProfile::OtaBundle),
        b"ota-fallback" => Some(RuntimeProfile::OtaFallback),
        b"net" => Some(RuntimeProfile::Net),
        b"none" => Some(RuntimeProfile::None),
        b"reset" => Some(RuntimeProfile::Reset),
        _ => None,
    }
}

#[must_use]
fn trim_ascii_token(bytes: &[u8]) -> &[u8] {
    let mut start = 0usize;
    let mut end = bytes.len();
    while start < end && is_trim_byte(bytes[start]) {
        start += 1;
    }
    while end > start && is_trim_byte(bytes[end - 1]) {
        end -= 1;
    }
    &bytes[start..end]
}

#[must_use]
const fn is_trim_byte(byte: u8) -> bool {
    matches!(byte, 0 | b' ' | b'\n' | b'\r' | b'\t')
}

#[cfg(test)]
mod tests {
    use super::{parse_runtime_mode, parse_runtime_profile, RuntimeMode, RuntimeProfile};

    #[test]
    fn parse_runtime_mode_tokens() {
        assert_eq!(parse_runtime_mode(b"proof"), Some(RuntimeMode::Proof));
        assert_eq!(
            parse_runtime_mode(b" interactive-minimal\n"),
            Some(RuntimeMode::InteractiveMinimal)
        );
        assert_eq!(parse_runtime_mode(b"interactive-full\0"), Some(RuntimeMode::InteractiveFull));
        assert_eq!(parse_runtime_mode(b"invalid"), None);
    }

    #[test]
    fn parse_runtime_profile_tokens() {
        assert_eq!(parse_runtime_profile(b"full"), Some(RuntimeProfile::Full));
        assert_eq!(parse_runtime_profile(b"bringup\r\n"), Some(RuntimeProfile::Bringup));
        assert_eq!(parse_runtime_profile(b"quick"), Some(RuntimeProfile::Quick));
        assert_eq!(parse_runtime_profile(b"ota"), Some(RuntimeProfile::Ota));
        assert_eq!(parse_runtime_profile(b"ota-flip"), Some(RuntimeProfile::OtaFlip));
        assert_eq!(parse_runtime_profile(b"ota-fallback"), Some(RuntimeProfile::OtaFallback));
        assert_eq!(parse_runtime_profile(b"net"), Some(RuntimeProfile::Net));
        assert_eq!(parse_runtime_profile(b"none"), Some(RuntimeProfile::None));
        assert_eq!(parse_runtime_profile(b"bogus"), None);
    }
}
