// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Narrow parser for `policies/update-trust.toml` (RFC-0089 §4 device trust
// anchor). Shared between the build script (which PANICS on any Err so a
// half-parsed trust list can never produce a permissive anchor) and the host
// test suite (which asserts the reject cases) via `include!`/`#[path]` — the
// build script itself is not unit-testable, the shared function is.
//
// Format: `[[publisher]]` blocks, each with exactly one
// `pubkey = "<64 lowercase hex>"`. Comments (`#`) and blank lines allowed.
// Anything else is an error.

/// Parses the trust file into raw 32-byte Ed25519 verifying keys.
pub fn parse_update_trust(text: &str) -> Result<Vec<[u8; 32]>, String> {
    let mut keys: Vec<[u8; 32]> = Vec::new();
    let mut pending: Option<[u8; 32]> = None;
    let mut saw_block = false;

    let flush = |pending: &mut Option<[u8; 32]>,
                 keys: &mut Vec<[u8; 32]>,
                 saw_block: bool|
     -> Result<(), String> {
        match pending.take() {
            Some(key) => {
                if keys.contains(&key) {
                    return Err("duplicate publisher pubkey".into());
                }
                keys.push(key);
                Ok(())
            }
            None if !saw_block => Ok(()),
            None => Err("[[publisher]] block missing pubkey".into()),
        }
    };

    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if line == "[[publisher]]" {
            flush(&mut pending, &mut keys, saw_block)?;
            saw_block = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("pubkey") {
            if !saw_block {
                return Err("pubkey outside a [[publisher]] block".into());
            }
            let hex = value.trim_start_matches(['=', ' ']).trim().trim_matches('"');
            if hex.len() != 64
                || !hex.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                return Err(format!("pubkey must be 64 lowercase hex chars, got `{hex}`"));
            }
            let mut key = [0u8; 32];
            for (i, chunk) in key.iter_mut().enumerate() {
                *chunk = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
                    .map_err(|_| "hex re-parse failed (validated above)".to_string())?;
            }
            if pending.replace(key).is_some() {
                return Err("duplicate pubkey in one [[publisher]] block".into());
            }
            continue;
        }
        return Err(format!("unrecognized line `{line}` (format is narrow on purpose)"));
    }
    flush(&mut pending, &mut keys, saw_block)?;
    if keys.is_empty() {
        return Err("no publishers — an empty anchor bricks every update path".into());
    }
    Ok(keys)
}
