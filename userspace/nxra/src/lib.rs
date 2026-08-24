// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]

//! CONTEXT: `.nxra` signed recovery action tokens (RFC-0088, TASK-0053).
//! ONE fixed 136-byte layout authorizes exactly ONE mutating recovery
//! action for a sender standing policy would deny — additive break-glass,
//! never a weakening of the standing paths. This crate owns the format:
//! bounded parse, the full deterministic verification pipeline (trust →
//! signature → action/arg → time window → replay high-water mark) with
//! STABLE reject reasons (audit + markers consume the labels verbatim),
//! and the host-side signer (`sign`, deterministic Ed25519 — no RNG).
//! Replay protection is a per-(verifier, key) monotone high-water mark:
//! bounded by construction, no nonce GC; the CALLER persists the new hwm
//! BEFORE executing the action (consume-before-act, RFC-0088).
//! OWNERS: @security @reliability
//! STATUS: Functional (host-first; OS enforcement lands with TASK-0053 P3)
//! API_STABILITY: Unstable (format v1 is the contract surface)
//! TEST_COVERAGE: unit tests below + tests/nxra_host (DoD matrix)
//! ADR: docs/rfcs/RFC-0088-nxra-signed-recovery-action-tokens.md

extern crate alloc;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};

/// Fixed wire length of a v1 token.
pub const TOKEN_LEN: usize = 136;
/// Format version byte.
pub const VERSION: u8 = 1;
/// Signed span: everything before the signature.
const SIGNED_LEN: usize = 72;
const MAGIC: [u8; 4] = *b"NXRA";
/// `flags` bit0: the not_before/not_after window is present.
const FLAG_TIME_WINDOW: u16 = 1;

/// Recovery action a token may authorize (RFC-0088 §format).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    SlotSwitch,
    TargetSet,
    FsckRepair,
    Reset,
}

impl Action {
    pub fn from_u8(raw: u8) -> Option<Self> {
        match raw {
            1 => Some(Self::SlotSwitch),
            2 => Some(Self::TargetSet),
            3 => Some(Self::FsckRepair),
            4 => Some(Self::Reset),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        match self {
            Self::SlotSwitch => 1,
            Self::TargetSet => 2,
            Self::FsckRepair => 3,
            Self::Reset => 4,
        }
    }

    /// Stable label (markers, `nx recovery token show`).
    pub fn label(self) -> &'static str {
        match self {
            Self::SlotSwitch => "slot-switch",
            Self::TargetSet => "target-set",
            Self::FsckRepair => "fsck-repair",
            Self::Reset => "reset",
        }
    }
}

/// Deterministic reject reasons — the labels ARE the audit/marker contract
/// (`bootctld: nxra reject (reason=<label>)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reject {
    Malformed,
    UnknownVersion,
    UntrustedKey,
    BadSignature,
    ActionDenied,
    Replay,
    Expired,
    NotYetValid,
    NoClock,
}

impl Reject {
    pub fn label(self) -> &'static str {
        match self {
            Self::Malformed => "malformed",
            Self::UnknownVersion => "unknown-version",
            Self::UntrustedKey => "untrusted-key",
            Self::BadSignature => "bad-signature",
            Self::ActionDenied => "action-denied",
            Self::Replay => "replay",
            Self::Expired => "expired",
            Self::NotYetValid => "not-yet-valid",
            Self::NoClock => "no-clock",
        }
    }
}

/// Decoded (structurally valid) token. Holding one does NOT imply trust —
/// only [`verify`] establishes authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub action: Action,
    pub seq: u64,
    /// `(not_before_ns, not_after_ns)` when the window flag is set.
    pub window: Option<(u64, u64)>,
    pub arg: u64,
    pub pubkey: [u8; 32],
}

/// One trusted signer: public key + the actions it may authorize.
/// Provisioned at build time (`policies/nxra-trust.toml` bake, Phase 2).
#[derive(Debug, Clone, Copy)]
pub struct TrustEntry {
    pub pubkey: [u8; 32],
    pub actions: &'static [Action],
}

// Build-time trust anchor (policies/nxra-trust.toml → build.rs codegen).
include!(concat!(env!("OUT_DIR"), "/trust_baked.rs"));

/// Bounded structural parse: exact length, magic, version, reserved flags.
/// No trust or crypto — [`verify`] runs the full pipeline.
pub fn parse(bytes: &[u8]) -> Result<Token, Reject> {
    if bytes.len() != TOKEN_LEN {
        return Err(Reject::Malformed);
    }
    if bytes[0..4] != MAGIC {
        return Err(Reject::Malformed);
    }
    if bytes[4] != VERSION {
        return Err(Reject::UnknownVersion);
    }
    let action = Action::from_u8(bytes[5]).ok_or(Reject::Malformed)?;
    let flags = u16::from_le_bytes([bytes[6], bytes[7]]);
    if flags & !FLAG_TIME_WINDOW != 0 {
        return Err(Reject::Malformed);
    }
    let seq = u64_at(bytes, 8);
    let not_before = u64_at(bytes, 16);
    let not_after = u64_at(bytes, 24);
    let window = if flags & FLAG_TIME_WINDOW != 0 {
        if not_after <= not_before {
            return Err(Reject::Malformed);
        }
        Some((not_before, not_after))
    } else {
        if not_before != 0 || not_after != 0 {
            return Err(Reject::Malformed);
        }
        None
    };
    let arg = u64_at(bytes, 32);
    let mut pubkey = [0u8; 32];
    pubkey.copy_from_slice(&bytes[40..72]);
    Ok(Token { action, seq, window, arg, pubkey })
}

/// Full verification pipeline (RFC-0088 order — the FIRST failing stage
/// names the reject): parse → trust → signature → action → arg → window →
/// replay. `hwm` is the persisted high-water mark for `(verifier, key)`;
/// `now_ns` is the trusted wall clock or `None` where none exists.
/// On success the caller MUST persist `token.seq` as the new hwm BEFORE
/// executing the action (consume-before-act).
pub fn verify(
    bytes: &[u8],
    trust: &[TrustEntry],
    requested: Action,
    requested_arg: u64,
    hwm: u64,
    now_ns: Option<u64>,
) -> Result<Token, Reject> {
    let token = parse(bytes)?;
    let entry =
        trust.iter().find(|entry| entry.pubkey == token.pubkey).ok_or(Reject::UntrustedKey)?;
    let key = VerifyingKey::from_bytes(&token.pubkey).map_err(|_| Reject::UntrustedKey)?;
    let signature =
        Signature::from_slice(&bytes[SIGNED_LEN..TOKEN_LEN]).map_err(|_| Reject::BadSignature)?;
    key.verify(&bytes[..SIGNED_LEN], &signature).map_err(|_| Reject::BadSignature)?;
    if token.action != requested || !entry.actions.contains(&token.action) {
        return Err(Reject::ActionDenied);
    }
    if token.arg != requested_arg {
        return Err(Reject::ActionDenied);
    }
    if let Some((not_before, not_after)) = token.window {
        let Some(now) = now_ns else { return Err(Reject::NoClock) };
        if now < not_before {
            return Err(Reject::NotYetValid);
        }
        if now >= not_after {
            return Err(Reject::Expired);
        }
    }
    if token.seq <= hwm {
        return Err(Reject::Replay);
    }
    Ok(token)
}

/// Signs a v1 token (host side, `nx recovery token make`). Deterministic
/// Ed25519 — no RNG involved, safe everywhere the crate builds.
pub fn sign(
    action: Action,
    seq: u64,
    window: Option<(u64, u64)>,
    arg: u64,
    signing_key: &[u8; 32],
) -> [u8; TOKEN_LEN] {
    use ed25519_dalek::Signer;
    let key = ed25519_dalek::SigningKey::from_bytes(signing_key);
    let mut out = [0u8; TOKEN_LEN];
    out[0..4].copy_from_slice(&MAGIC);
    out[4] = VERSION;
    out[5] = action.as_u8();
    let flags: u16 = if window.is_some() { FLAG_TIME_WINDOW } else { 0 };
    out[6..8].copy_from_slice(&flags.to_le_bytes());
    out[8..16].copy_from_slice(&seq.to_le_bytes());
    let (not_before, not_after) = window.unwrap_or((0, 0));
    out[16..24].copy_from_slice(&not_before.to_le_bytes());
    out[24..32].copy_from_slice(&not_after.to_le_bytes());
    out[32..40].copy_from_slice(&arg.to_le_bytes());
    out[40..72].copy_from_slice(key.verifying_key().as_bytes());
    let signature = key.sign(&out[..SIGNED_LEN]);
    out[SIGNED_LEN..TOKEN_LEN].copy_from_slice(&signature.to_bytes());
    out
}

/// First 8 lowercase-hex chars of the public key — the stable key id used
/// in markers and hwm record names (`nxra.hwm.<keyid8>`).
pub fn keyid8(pubkey: &[u8; 32]) -> [u8; 8] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = [0u8; 8];
    for i in 0..4 {
        out[i * 2] = HEX[(pubkey[i] >> 4) as usize];
        out[i * 2 + 1] = HEX[(pubkey[i] & 0x0f) as usize];
    }
    out
}

fn u64_at(bytes: &[u8], at: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[at..at + 8]);
    u64::from_le_bytes(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7u8; 32];
    const ALL: &[Action] =
        &[Action::SlotSwitch, Action::TargetSet, Action::FsckRepair, Action::Reset];

    fn trust() -> [TrustEntry; 1] {
        use ed25519_dalek::SigningKey;
        let pubkey = *SigningKey::from_bytes(&KEY).verifying_key().as_bytes();
        [TrustEntry { pubkey, actions: ALL }]
    }

    #[test]
    fn roundtrip_verifies_and_reports_fields() {
        let bytes = sign(Action::SlotSwitch, 5, None, 2, &KEY);
        let token = verify(&bytes, &trust(), Action::SlotSwitch, 2, 4, None).expect("verify");
        assert_eq!(token.seq, 5);
        assert_eq!(token.arg, 2);
        assert_eq!(token.window, None);
    }

    #[test]
    fn test_reject_pipeline_orders_are_stable() {
        let bytes = sign(Action::Reset, 9, None, 0, &KEY);
        // Length / magic / version / unknown action / reserved flags.
        assert_eq!(parse(&bytes[..10]), Err(Reject::Malformed));
        let mut bad = bytes;
        bad[0] = b'X';
        assert_eq!(parse(&bad), Err(Reject::Malformed));
        let mut bad = bytes;
        bad[4] = 9;
        assert_eq!(parse(&bad), Err(Reject::UnknownVersion));
        let mut bad = bytes;
        bad[5] = 0xEE;
        assert_eq!(parse(&bad), Err(Reject::Malformed));
        let mut bad = bytes;
        bad[6] = 0x02;
        assert_eq!(parse(&bad), Err(Reject::Malformed));
        // Untrusted key beats bad signature (trust lookup first).
        assert_eq!(verify(&bytes, &[], Action::Reset, 0, 0, None), Err(Reject::UntrustedKey));
        // Tamper inside the signed span.
        let mut tampered = bytes;
        tampered[33] ^= 0xFF;
        assert_eq!(
            verify(&tampered, &trust(), Action::Reset, 0, 0, None),
            Err(Reject::BadSignature)
        );
        // Wrong action / wrong arg.
        assert_eq!(
            verify(&bytes, &trust(), Action::SlotSwitch, 0, 0, None),
            Err(Reject::ActionDenied)
        );
        assert_eq!(verify(&bytes, &trust(), Action::Reset, 7, 0, None), Err(Reject::ActionDenied));
        // Replay: seq must EXCEED the high-water mark.
        assert_eq!(verify(&bytes, &trust(), Action::Reset, 0, 9, None), Err(Reject::Replay));
    }

    #[test]
    fn test_reject_window_edges_and_no_clock() {
        let bytes = sign(Action::TargetSet, 3, Some((100, 200)), 1, &KEY);
        let t = &trust();
        assert_eq!(verify(&bytes, t, Action::TargetSet, 1, 0, None), Err(Reject::NoClock));
        assert_eq!(verify(&bytes, t, Action::TargetSet, 1, 0, Some(99)), Err(Reject::NotYetValid));
        assert_eq!(verify(&bytes, t, Action::TargetSet, 1, 0, Some(200)), Err(Reject::Expired));
        assert!(verify(&bytes, t, Action::TargetSet, 1, 0, Some(150)).is_ok());
        // Windowless tokens with nonzero window bytes are malformed.
        let mut bad = sign(Action::TargetSet, 3, None, 1, &KEY);
        bad[16] = 1;
        assert_eq!(parse(&bad), Err(Reject::Malformed));
        // Inverted window never parses.
        let inverted = sign(Action::TargetSet, 3, Some((200, 100)), 1, &KEY);
        assert_eq!(parse(&inverted), Err(Reject::Malformed));
    }

    #[test]
    fn test_reject_action_outside_key_allowlist() {
        use ed25519_dalek::SigningKey;
        let pubkey = *SigningKey::from_bytes(&KEY).verifying_key().as_bytes();
        let narrow = [TrustEntry { pubkey, actions: &[Action::FsckRepair] }];
        let bytes = sign(Action::Reset, 1, None, 0, &KEY);
        assert_eq!(verify(&bytes, &narrow, Action::Reset, 0, 0, None), Err(Reject::ActionDenied));
    }

    #[test]
    fn keyid_is_stable_hex_prefix() {
        let mut pubkey = [0u8; 32];
        pubkey[0] = 0xAB;
        pubkey[1] = 0x01;
        pubkey[2] = 0xFF;
        pubkey[3] = 0x00;
        assert_eq!(&keyid8(&pubkey), b"ab01ff00");
    }
}
