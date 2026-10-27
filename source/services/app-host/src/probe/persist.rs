// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the `@persist` round trip of a restorable app (TASK-0333 6e-1, RFC-0100 §6,
//! ADR-0086). The DSL runtime owns the codec (`NXPS` v1, name-keyed, per-entry tolerant); THIS
//! module owns where the bytes live: the app's own record in statefsd (`/state/app/@self/persist`
//! over the `svc.state` route execd provisioned iff the manifest said `restorable = true`; the
//! `@self` token is the sender's kernel identity, resolved by statefsd). The snapshot is written
//! when the compositor says the window is HIDDEN (the lifecycle grace runs; the authority may
//! freeze and memd may kill the process after it) and when the window closes; it is read ONCE at
//! mount, before the initial effects, so service-loaded data still refreshes over the stale copy.
//! Bounded both ways: a snapshot over `MAX_SNAPSHOT_BYTES` is not written (and said), a reply
//! over the buffer is not read. An app without the route sees `None` and nothing else — the
//! route is the capability.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: codec in `nexus_dsl_runtime::persist::tests`; the chain on the usb-visible
//! lane (`apphost: persist saved (…)`, `SELFTEST: app restore ok (…)`)

use super::{raw_marker, DslApp};
use crate::svc_call::call_reply;
use alloc::vec::Vec;
use statefs::protocol as proto;

/// The app's own record (statefsd rewrites `@self` to the sender's identity).
const KEY: &str = "/state/app/@self/persist";
/// The most a snapshot may weigh: durable SMALL state (a few fields), never a document.
const MAX_SNAPSHOT_BYTES: usize = 2048;
/// The reply buffer: the GET header (9 bytes, 17 with a nonce) and the snapshot.
const REPLY_BUF: usize = MAX_SNAPSHOT_BYTES + 64;

/// Why a snapshot is written — the marker names it.
#[derive(Clone, Copy)]
pub(super) enum Reason {
    /// The compositor said the window is hidden (the grace before a freeze).
    Hidden,
    /// The window closed; the process exits right after.
    Close,
}

impl Reason {
    const fn name(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Close => "close",
        }
    }
}

/// The `svc.state` SEND slot, if the launcher provisioned it (`restorable = true`).
fn state_slot() -> Option<u32> {
    nexus_sdk_routes::route_for_svc("state").map(|r| r.child_slot)
}

/// The stored snapshot, if the app is restorable and has one. Silent when the route is absent
/// (not restorable), the record is missing (first launch) or the reply is malformed.
pub(super) fn load() -> Option<Vec<u8>> {
    let slot = state_slot()?;
    let req = proto::encode_key_only_request(proto::OP_GET, KEY).ok()?;
    let mut resp = [0u8; REPLY_BUF];
    let len = call_reply(slot, &req, &mut resp)?;
    match proto::decode_get_response(&resp[..len]) {
        Ok(bytes) if !bytes.is_empty() && bytes.len() <= MAX_SNAPSHOT_BYTES => Some(bytes),
        Ok(_) => None,
        Err(statefs::StatefsError::NotFound) => None,
        Err(_) => {
            raw_marker("apphost: persist load FAIL (bad reply)");
            None
        }
    }
}

/// Writes the runtime's `@persist` snapshot (nothing when the program has no persisted field
/// or the app is not restorable). The marker says how many fields and bytes went.
pub(super) fn save(runtime: &nexus_dsl_runtime::Runtime<'_>, reason: Reason) {
    let Some(slot) = state_slot() else { return };
    let Some(bytes) = runtime.persist_snapshot() else { return };
    if bytes.len() > MAX_SNAPSHOT_BYTES {
        raw_marker(&alloc::format!(
            "apphost: persist FAIL (snapshot {} bytes over {MAX_SNAPSHOT_BYTES})",
            bytes.len()
        ));
        return;
    }
    let fields = entries(&bytes);
    let Ok(req) = proto::encode_put_request(KEY, &bytes) else { return };
    let mut resp = [0u8; 64];
    let Some(len) = call_reply(slot, &req, &mut resp) else {
        raw_marker("apphost: persist FAIL (state route unreachable)");
        return;
    };
    match proto::decode_status_response(proto::OP_PUT, &resp[..len]) {
        Ok(proto::STATUS_OK) => raw_marker(&alloc::format!(
            "apphost: persist saved (fields={fields} bytes={} reason={})",
            bytes.len(),
            reason.name()
        )),
        Ok(status) => {
            raw_marker(&alloc::format!("apphost: persist FAIL (status={status})"));
        }
        Err(_) => raw_marker("apphost: persist FAIL (bad reply)"),
    }
}

/// Writes the snapshot of a mounted app; nothing without one.
pub(super) fn save_app(app: Option<&DslApp>, reason: Reason) {
    if let Some(dsl) = app {
        save(&dsl.view.runtime, reason);
    }
}

/// The compositor's sighting of THIS window (`OP_SURFACE_SEEN`, ADR-0086): `true` when `frame`
/// was one. Hidden = the lifecycle grace runs — the `@persist` fields go to the app's own record
/// now, before the authority freezes the process and memd may kill it.
pub(super) fn absorb_seen(frame: &[u8], app: Option<&DslApp>) -> bool {
    use nexus_display_proto::surface_seen::{decode_surface_seen, SEEN_HIDDEN};
    let Some(seen) = decode_surface_seen(frame) else { return false };
    if seen == SEEN_HIDDEN {
        save_app(app, Reason::Hidden);
    }
    true
}

/// The entry count a snapshot header carries (`NXPS`, version, `u16le`).
pub(super) fn entries(snapshot: &[u8]) -> u16 {
    match snapshot.get(5..7) {
        Some(&[lo, hi]) => u16::from_le_bytes([lo, hi]),
        _ => 0,
    }
}
