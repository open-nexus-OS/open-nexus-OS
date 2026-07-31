// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Reply-frame DECODING for the app-host effect adapters — the
//! length-prefixed bodies bundlemgrd and sessiond answer with. Split out of
//! `effect_host.rs` (module-size ratchet): dispatching an effect and decoding
//! a wire frame are different jobs, and the decoders are the part that has to
//! be fail-CLOSED.
//!
//! The rule these share: a frame that is not recognisably OUR reply must be
//! reported as such, never as an empty result. `parse_app_entries` used to
//! return an empty Vec for a foreign frame, which the shell read as "the OS
//! has no apps" and answered by dropping every desktop icon — silently.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: mirrors windowd's host-tested `from_list_apps_response`

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]

use alloc::string::String;
use alloc::vec::Vec;

/// The `OP_LIST_APPS` body: `[id_len,id, label_len,label, icon_len,icon]` per
/// entry.
///
/// `None` = NOT a valid OK listing (wrong envelope, error status, short body).
/// An empty Vec made "a foreign frame landed in our reply slot" look exactly
/// like "the OS has no apps", and the shell answered by dropping every desktop
/// icon with nothing in the log to say why.
pub(crate) fn parse_app_entries(frame: &[u8]) -> Option<Vec<(String, String, String)>> {
    let (status, count) = nexus_abi::bundlemgrd::decode_list_apps_header(frame)?;
    if status != nexus_abi::bundlemgrd::STATUS_OK {
        return None;
    }
    let mut out = Vec::new();
    let mut pos = nexus_abi::bundlemgrd::LIST_APPS_BODY_OFFSET;
    for _ in 0..count {
        // A short body is TRUNCATION, not "fewer apps" — fail the reply.
        out.push((
            take_lp_str(frame, &mut pos)?,
            take_lp_str(frame, &mut pos)?,
            take_lp_str(frame, &mut pos)?,
        ));
    }
    Some(out)
}

/// Parses the sessiond `OP_GET_STATE` response into user DISPLAY NAMES. Each
/// entry is `[id_len, id, name_len, name, product_len, product]`; we keep the
/// name (the greeter renders it). Fail-soft like [`parse_app_entries`].
/// The registered users as `(id, display_name)`. `login` takes the id; the
/// UI shows the name.
pub(crate) fn parse_session_users(frame: &[u8]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some((status, _state, _active, count)) =
        nexus_abi::sessiond::decode_get_state_header(frame)
    else {
        return out;
    };
    if status != nexus_abi::sessiond::STATUS_OK {
        return out;
    }
    let mut pos = nexus_abi::sessiond::GET_STATE_BODY_OFFSET;
    for _ in 0..count {
        let id = take_lp_str(frame, &mut pos);
        let name = take_lp_str(frame, &mut pos);
        let product = take_lp_str(frame, &mut pos);
        match (id, name, product) {
            (Some(id), Some(name), Some(_)) => out.push((id, name)),
            _ => break,
        }
    }
    out
}

/// The id of the user at sessiond's `active_idx` — its answer to "who logs in
/// if nobody picks". `None` when there are no users or the frame is short.
pub(crate) fn parse_session_active(frame: &[u8]) -> Option<String> {
    let (status, _state, active, count) = nexus_abi::sessiond::decode_get_state_header(frame)?;
    if status != nexus_abi::sessiond::STATUS_OK || count == 0 {
        return None;
    }
    let users = parse_session_users(frame);
    let idx = (active as usize).min(users.len().saturating_sub(1));
    users.into_iter().nth(idx).map(|(id, _)| id)
}

/// Reads a `[len:u8, bytes…]` UTF-8 string, advancing `pos`. `None` on a short
/// frame or invalid UTF-8 (the bound the callers stop on).
pub(crate) fn take_lp_str(frame: &[u8], pos: &mut usize) -> Option<String> {
    let len = *frame.get(*pos)? as usize;
    let start = pos.checked_add(1)?;
    let end = start.checked_add(len)?;
    let bytes = frame.get(start..end)?;
    *pos = end;
    core::str::from_utf8(bytes).ok().map(String::from)
}
