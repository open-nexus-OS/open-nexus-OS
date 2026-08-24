// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: `nx recovery token make/show` (TASK-0053, RFC-0088) — the host
//! half of `.nxra` break-glass: `make` signs one token from a 32-byte seed
//! file (deterministic Ed25519, no RNG), `show` decodes untrusted bytes
//! and reports the verdict against the IMAGE-BAKED trust anchor (the same
//! `policies/nxra-trust.toml` the verifiers link). nx exit classes stay
//! the CLI contract; the token verdict is data.
//! OWNERS: @security @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: tests/recovery_cli.rs
//! ADR: docs/rfcs/RFC-0088-nxra-signed-recovery-action-tokens.md

use serde_json::json;

use crate::cli::{RecoveryArgs, RecoveryTokenAction, TokenMakeArgs, TokenShowArgs};
use crate::error::{ExecResult, ExitClass, NxError};

pub(crate) fn handle_recovery(args: RecoveryArgs) -> ExecResult {
    match args.action {
        RecoveryTokenAction::Make(make) => handle_make(make),
        RecoveryTokenAction::Show(show) => handle_show(show),
    }
}

fn parse_action(label: &str) -> Option<nxra::Action> {
    match label {
        "slot-switch" => Some(nxra::Action::SlotSwitch),
        "target-set" => Some(nxra::Action::TargetSet),
        "fsck-repair" => Some(nxra::Action::FsckRepair),
        "reset" => Some(nxra::Action::Reset),
        _ => None,
    }
}

fn handle_make(args: TokenMakeArgs) -> ExecResult {
    let Some(action) = parse_action(&args.action) else {
        return Err(NxError::new(
            ExitClass::Usage,
            format!("recovery: unknown action `{}`", args.action),
        ));
    };
    let window = match (args.not_before, args.not_after) {
        (None, None) => None,
        (Some(nb), Some(na)) if na > nb => Some((nb, na)),
        _ => {
            return Err(NxError::new(
                ExitClass::Usage,
                "recovery: --not-before/--not-after come together, after > before",
            ))
        }
    };
    let seed_hex = std::fs::read_to_string(&args.key).map_err(|err| {
        NxError::new(
            ExitClass::MissingDependency,
            format!("recovery: read {}: {err}", args.key.display()),
        )
    })?;
    let seed = decode_hex32(seed_hex.trim()).ok_or_else(|| {
        NxError::new(ExitClass::ValidationReject, "recovery: key file must hold 64 hex chars")
    })?;
    let token = nxra::sign(action, args.seq, window, args.arg, &seed);
    std::fs::write(&args.out, token).map_err(|err| {
        NxError::new(ExitClass::Internal, format!("recovery: write {}: {err}", args.out.display()))
    })?;
    let pubkey = *ed25519_dalek::SigningKey::from_bytes(&seed).verifying_key().as_bytes();
    let data = json!({
        "out": args.out.display().to_string(),
        "keyid": String::from_utf8_lossy(&nxra::keyid8(&pubkey)),
        "action": action.label(),
        "seq": args.seq,
        "arg": args.arg,
        "window": window.map(|(nb, na)| json!({ "not_before_ns": nb, "not_after_ns": na })),
    });
    Ok((
        ExitClass::Success,
        format!("recovery: token written to {}", args.out.display()),
        args.json,
        Some(data),
    ))
}

fn handle_show(args: TokenShowArgs) -> ExecResult {
    let bytes = std::fs::read(&args.path).map_err(|err| {
        NxError::new(
            ExitClass::MissingDependency,
            format!("recovery: read {}: {err}", args.path.display()),
        )
    })?;
    let token = match nxra::parse(&bytes) {
        Ok(token) => token,
        Err(reject) => {
            return Err(NxError::new(
                ExitClass::ValidationReject,
                format!("recovery: token rejected (reason={})", reject.label()),
            ))
        }
    };
    // Verdict against the image-baked anchor: replay state lives on the
    // device, so hwm=0 here — `verdict` covers trust/signature/action.
    let verdict = match nxra::verify(&bytes, nxra::BAKED_TRUST, token.action, token.arg, 0, None) {
        Ok(_) => "trusted",
        Err(nxra::Reject::NoClock) => "trusted (window needs a device clock)",
        Err(reject) => reject.label(),
    };
    let data = json!({
        "keyid": String::from_utf8_lossy(&nxra::keyid8(&token.pubkey)),
        "action": token.action.label(),
        "seq": token.seq,
        "arg": token.arg,
        "window": token.window.map(|(nb, na)| json!({ "not_before_ns": nb, "not_after_ns": na })),
        "verdict": verdict,
    });
    Ok((
        ExitClass::Success,
        format!("recovery: {} token (verdict={verdict})", token.action.label()),
        args.json,
        Some(data),
    ))
}

fn decode_hex32(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let s = core::str::from_utf8(chunk).ok()?;
        out[i] = u8::from_str_radix(s, 16).ok()?;
    }
    Some(out)
}
