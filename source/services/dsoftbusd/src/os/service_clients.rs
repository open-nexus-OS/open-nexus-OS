//! CONTEXT: Cached client-slot helpers for local service IPC.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered transitively by dsoftbusd QEMU proofs.
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use core::sync::atomic::{AtomicU32, Ordering};

use nexus_ipc::KernelClient;

pub(crate) static SAMGRD_SEND_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
pub(crate) static SAMGRD_RECV_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
pub(crate) static BUNDLEMGRD_SEND_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
pub(crate) static BUNDLEMGRD_RECV_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
pub(crate) static PACKAGEFSD_SEND_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
pub(crate) static PACKAGEFSD_RECV_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
pub(crate) static LOGD_SEND_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);
pub(crate) static LOGD_RECV_SLOT_CACHE: AtomicU32 = AtomicU32::new(0);

pub(crate) fn invalidate_cached_slots(send_slot: &AtomicU32, recv_slot: &AtomicU32) {
    send_slot.store(0, Ordering::Relaxed);
    recv_slot.store(0, Ordering::Relaxed);
}

pub(crate) fn cached_client_slots(
    target: &str,
    send_slot: &AtomicU32,
    recv_slot: &AtomicU32,
    force_refresh: bool,
) -> Option<KernelClient> {
    if !force_refresh {
        let cached_send = send_slot.load(Ordering::Relaxed);
        let cached_recv = recv_slot.load(Ordering::Relaxed);
        if cached_send != 0 && cached_recv != 0 {
            if let Ok(client) = KernelClient::new_with_slots(cached_send, cached_recv) {
                return Some(client);
            }
            invalidate_cached_slots(send_slot, recv_slot);
        }
    }
    let client = match KernelClient::new_for(target) {
        Ok(client) => {
            if target == "packagefsd" {
                // #region agent log
                let _ = nexus_abi::debug_println("dbg:dsoftbusd: packagefsd client new_for ok");
                // #endregion
            }
            client
        }
        Err(_) => {
            if target == "packagefsd" {
                // #region agent log
                let _ = nexus_abi::debug_println("dbg:dsoftbusd: packagefsd client new_for fail");
                // #endregion
            }
            return None;
        }
    };
    let (new_send, new_recv) = client.slots();
    send_slot.store(new_send, Ordering::Relaxed);
    recv_slot.store(new_recv, Ordering::Relaxed);
    Some(client)
}

pub(crate) fn cached_client_slots_bounded(
    target: &str,
    send_slot: &AtomicU32,
    recv_slot: &AtomicU32,
    attempts: usize,
) -> Option<KernelClient> {
    if let Some(client) = cached_client_slots(target, send_slot, recv_slot, false) {
        return Some(client);
    }
    for _ in 0..attempts {
        if let Some(client) = cached_client_slots(target, send_slot, recv_slot, true) {
            return Some(client);
        }
        let _ = nexus_abi::yield_();
    }
    if target == "packagefsd" {
        // #region agent log
        let _ = nexus_abi::debug_println("dbg:dsoftbusd: packagefsd client bounded fail");
        // #endregion
    }
    None
}
