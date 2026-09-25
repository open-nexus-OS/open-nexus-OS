// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The ONE readiness point of an OS service (RFC-0093 §2, ADR-0062): print the
//! `<svc>: ready` marker and announce `@ready` to init on the control channel. Split out of
//! `lib.rs` (module-size ratchet).
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P2)
//! API_STABILITY: Stable (`nexus_service_entry::ready`)
//! TEST_COVERAGE: QEMU ladders (`init: up <svc>` follows `<svc>: ready`), scripts/check-init-sync.sh

use super::{debug_write_byte, debug_write_bytes, debug_write_str, service_name};

/// The ONE readiness point of a service (RFC-0093 §2, ADR-0062): prints the service's
/// `<svc>: ready` marker through the folding-aware `debug_println` AND announces `@ready`
/// to init on the control channel, so `init: up <svc>` is init's observation of THIS call
/// and nothing else. The announce is one-way (no reply frame may land in the child's
/// control RSP queue — a stale frame there is exactly the confused-waiter class routing v2
/// removes). A failed announce is loud: init would never print `init: up`, the ladder
/// fails, and the raw witness names the service.
pub fn ready(marker: &str) -> nexus_abi::SysResult<()> {
    let printed = nexus_abi::debug_println(marker);
    announce_ready();
    printed
}

/// `@ready` on the control REQ slot (`nexus_service_topology::CTRL_SLOTS.send`, installed by init in
/// every child — the same slot `@reply`/`@mint-pair` queries use). Routing-frame encoding, no nonce: init
/// records it and sends nothing back. The announce must NEVER hold the service back from
/// its serving loop: init's responder drains the control queue only after orchestration,
/// and orchestration itself waits on services (the block driver serves the system volume),
/// so waiting here for queue room is a circular wait (seen: blkd looping in `ready()`,
/// volume unavailable, init fatal). Hence: non-blocking attempts, a handful of yields at
/// most, loud on failure. The kernel's blocking send is avoided on purpose as well (it arms a
/// timer wakeup before the first attempt and does not disarm it on an immediate error).
fn announce_ready() {
    announce_verb(b"@ready", "ready");
}

/// Reports a boot stage to init (`@stage <label>`, RFC-0093 §2). Only windowd may report one —
/// init checks the SENDER's control channel, so a wrong sender is refused there, not trusted
/// here. Same one-way framing as `@ready`: init records it and sends nothing back.
pub fn stage(stage: nexus_service_topology::Stage) {
    /// `@stage ` + the longest label, well inside the routing frame's 48-byte name field
    /// (`nexus_service_topology::stage` proves the fit).
    const PREFIX: &[u8] = b"@stage ";
    let label = stage.label().as_bytes();
    let mut verb = [0u8; 32];
    let n = PREFIX.len() + label.len();
    if n > verb.len() {
        return;
    }
    verb[..PREFIX.len()].copy_from_slice(PREFIX);
    verb[PREFIX.len()..n].copy_from_slice(label);
    announce_verb(&verb[..n], "stage");
}

/// Blocks until this service's declared boot stage may begin (ADR-0062, RFC-0093 §3).
///
/// The service's own `ServiceSpec.stage` names the tier it belongs to; it waits for that tier's
/// PREREQUISITE on the boot-stage fence init pinned into its slot (WAIT rights only — it can
/// block here, never release a stage for anyone else). A process the topology does not declare
/// — a spawned app child — holds no fence and waits for nothing.
///
/// The fence decides the ORDER; the deadline only decides how long a broken boot may stay
/// SILENT. On the liveness bound the service names itself once and keeps waiting, so a stage
/// that never opens produces a witness instead of a fleet that quietly does nothing (the exact
/// failure the kernel's quiet-stall witness catches from the other side).
pub fn wait_for_stage() {
    /// A stage that has not opened after this long is a defect, not slow bring-up. Matches the
    /// liveness bounds init already uses for supervised peers.
    const LIVENESS_NS: u64 = 2_000_000_000;

    let Some(spec) = nexus_service_topology::spec_for(service_name().as_bytes()) else {
        return;
    };
    let prerequisite = spec.stage.prerequisite();
    if prerequisite == 0 {
        return;
    }
    let slot = nexus_service_topology::STAGE_FENCE_SLOT;
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(LIVENESS_NS);
    match nexus_abi::fence_wait(slot, prerequisite, deadline) {
        Ok(()) => return,
        // No fence in the slot (not an init child) or the kernel refused: nothing to wait on.
        Err(nexus_abi::AbiError::CapabilityDenied) | Err(nexus_abi::AbiError::InvalidArgument) => {
            return
        }
        Err(_) => {}
    }
    debug_write_bytes(b"FAIL stage wait svc=");
    debug_write_str(service_name());
    debug_write_bytes(b" stage=");
    debug_write_str(spec.stage.label());
    debug_write_byte(b'\n');
    // The barrier is the contract: keep waiting now that the wedge has a name.
    let _ = nexus_abi::fence_wait(slot, prerequisite, 0);
}

/// The ONE sender behind every one-way control verb: routing-frame encoding, no nonce, a
/// handful of NONBLOCK attempts, loud on failure. It must NEVER block — init's responder drains
/// the control queue only after orchestration, and orchestration itself waits on services, so a
/// blocking send here is a circular wait (seen: blkd looping in `ready()`, volume
/// unavailable, init fatal).
fn announce_verb(verb: &[u8], what: &str) {
    /// Non-blocking attempts before giving up (each separated by one `yield_()`).
    const ANNOUNCE_ATTEMPTS: u32 = 4;
    let mut buf = [0u8; 64];
    let Some(n) = nexus_abi::routing::encode_route_get(verb, &mut buf) else {
        return;
    };
    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, n as u32);
    let mut attempt = 0;
    let err = loop {
        match nexus_abi::ipc_send_v1(
            nexus_service_topology::CTRL_SLOTS.send,
            &hdr,
            &buf[..n],
            nexus_abi::IPC_SYS_NONBLOCK,
            0,
        ) {
            Ok(_) => return,
            Err(nexus_abi::IpcError::QueueFull) if attempt + 1 < ANNOUNCE_ATTEMPTS => {
                attempt += 1;
                let _ = nexus_abi::yield_();
            }
            Err(other) => break other,
        }
    };
    debug_write_bytes(b"FAIL ");
    debug_write_str(what);
    debug_write_bytes(b" announce svc=");
    debug_write_str(service_name());
    debug_write_bytes(b" err=");
    debug_write_str(match err {
        nexus_abi::IpcError::QueueFull => "queue-full",
        nexus_abi::IpcError::TimedOut => "timed-out",
        nexus_abi::IpcError::NoSuchEndpoint => "no-endpoint",
        nexus_abi::IpcError::PermissionDenied => "denied",
        nexus_abi::IpcError::PeerClosed => "peer-closed",
        _ => "other",
    });
    debug_write_byte(b'\n');
}
