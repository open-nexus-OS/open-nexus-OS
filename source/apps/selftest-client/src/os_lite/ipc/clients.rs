// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Per-target kernel-client slot cache and `cached_*_client()`
//! constructors. Extracted verbatim from the previous monolithic `os_lite`
//! block in `main.rs` (TASK-0023B / RFC-0038 phase 1, cut 3). No behavior,
//! marker, or reject-path change.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (binary crate)
//! TEST_COVERAGE: QEMU marker ladder via `just test-os` (full ladder).
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md, docs/rfcs/RFC-0038-*.md

use core::sync::atomic::{AtomicU32, Ordering};

use nexus_ipc::KernelClient;

static REPLY_SEND_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
static REPLY_RECV_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
static NETSTACKD_SEND_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
static NETSTACKD_RECV_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
static SAMGRD_SEND_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
static SAMGRD_RECV_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
static DSOFTBUSD_SEND_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
static DSOFTBUSD_RECV_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);

fn invalidate_client_cache(send_slot: &AtomicU32, recv_slot: &AtomicU32) {
    send_slot.store(0, Ordering::Relaxed);
    recv_slot.store(0, Ordering::Relaxed);
}

fn cached_client(
    target: &str,
    send_slot: &AtomicU32,
    recv_slot: &AtomicU32,
) -> core::result::Result<KernelClient, ()> {
    for force_refresh in [false, true] {
        if !force_refresh {
            let cached_send = send_slot.load(Ordering::Relaxed);
            let cached_recv = recv_slot.load(Ordering::Relaxed);
            if cached_send != 0 && cached_recv != 0 {
                if let Ok(client) = KernelClient::new_with_slots(cached_send, cached_recv) {
                    return Ok(client);
                }
                invalidate_client_cache(send_slot, recv_slot);
            }
        }
        if let Ok(client) = KernelClient::new_for(target) {
            let (new_send, new_recv) = client.slots();
            send_slot.store(new_send, Ordering::Relaxed);
            recv_slot.store(new_recv, Ordering::Relaxed);
            return Ok(client);
        }
        invalidate_client_cache(send_slot, recv_slot);
    }
    Err(())
}

pub(crate) fn cached_reply_client() -> core::result::Result<KernelClient, ()> {
    cached_client("@reply", &REPLY_SEND_SLOT_CACHE, &REPLY_RECV_SLOT_CACHE)
}

pub(crate) fn cached_netstackd_client() -> core::result::Result<KernelClient, ()> {
    cached_client("netstackd", &NETSTACKD_SEND_SLOT_CACHE, &NETSTACKD_RECV_SLOT_CACHE)
}

pub(crate) fn cached_samgrd_client() -> core::result::Result<KernelClient, ()> {
    cached_client("samgrd", &SAMGRD_SEND_SLOT_CACHE, &SAMGRD_RECV_SLOT_CACHE)
}

pub(crate) fn cached_dsoftbusd_client() -> core::result::Result<KernelClient, ()> {
    cached_client("dsoftbusd", &DSOFTBUSD_SEND_SLOT_CACHE, &DSOFTBUSD_RECV_SLOT_CACHE)
}
