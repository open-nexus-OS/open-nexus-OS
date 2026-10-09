// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: vfsd's whole-file VMO write (TASK-0068, RFC-0095) — the write
//! half of the VMO data plane `splice_os.rs` reads through. A message moves
//! ONE capability, so a write is two messages: `OP_ARM_VMO` moves the
//! client's VMO into the armed table (keyed by the KERNEL sender id — the
//! table bundlemgrd and packagefsd use, `nexus_ipc::armed_vmo`), and the
//! `OP_WRITE_VMO` that follows moves the reply cap. The content never gets a
//! buffer of its own here: nxfs pulls each chunk out of the VMO (`vmo_read`,
//! a kernel copy) straight into its one reused working buffer, inside ONE
//! transaction — no mapping, no unsafe, no allocation that scales with it.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: wire codec (vfs-types) + engine pull/rollback contract
//!   (nxfs `tests/write_from.rs`) on the host; this glue by the QEMU
//!   screencapd lane
//! ADR: docs/adr/0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md

extern crate alloc;

use alloc::vec::Vec;

use nexus_ipc::armed_vmo::{Armed, ArmedVmos};
use nexus_vfs_types::{fileops, VfsError};

use crate::os_lite::{debug_print, vmo_len};

/// Serves `OP_ARM_VMO`: keeps the moved VMO for the sender's next
/// `OP_WRITE_VMO`. There is no answer, so a refusal is a UART line — and the
/// VMO left over (the sender's previous one, or this one when the frame is
/// malformed or the table full) is closed here: no client VMO stays pinned
/// by a capability this service no longer tracks.
pub(crate) fn arm(armed: &mut ArmedVmos, frame: &[u8], sender: u64, vmo: Option<u32>) {
    let Some(vmo) = vmo else {
        debug_print("vfsd: FAIL arm vmo (no vmo cap)\n");
        return;
    };
    if frame.len() != 1 {
        debug_print("vfsd: FAIL arm vmo (malformed)\n");
        let _ = nexus_abi::cap_close(vmo);
        return;
    }
    match armed.arm(sender, vmo) {
        Armed::Stored => {}
        Armed::Replaced(old) => {
            let _ = nexus_abi::cap_close(old);
        }
        Armed::Full(vmo) => {
            debug_print("vfsd: FAIL arm vmo (table full)\n");
            let _ = nexus_abi::cap_close(vmo);
        }
    }
}

/// Serves `OP_WRITE_VMO` (`payload` = the frame after its opcode) from the
/// VMO this sender armed (`None`: it armed nothing) and returns the status
/// reply. The VMO is closed on EVERY path BEFORE the reply exists: a client
/// that destroys its VMO on the answer (`vmo_destroy` refuses while another
/// capability names it) must find this one gone.
pub(crate) fn write(
    store: Option<&mut nxfsd::DataStore>,
    payload: &[u8],
    vmo: Option<u32>,
) -> Vec<u8> {
    let reply = match (store, vmo) {
        (_, None) => status(VfsError::Invalid),
        (None, Some(_)) => nxfsd::write_unavailable(),
        (Some(store), Some(vmo)) => write_armed(store, payload, vmo),
    };
    if let Some(vmo) = vmo {
        let _ = nexus_abi::cap_close(vmo);
    }
    reply
}

/// The bounded write: decode, require a plain VMO that covers `len`, then
/// let nxfs pull the content chunk by chunk.
fn write_armed(store: &mut nxfsd::DataStore, payload: &[u8], vmo: u32) -> Vec<u8> {
    let Some((path, len)) = fileops::decode_write_vmo(payload) else {
        return status(VfsError::Invalid);
    };
    // `vmo_len` answers only for a plain VMO: a read-only alias (which
    // `vmo_read` refuses) or a non-VMO capability is `Invalid`.
    match vmo_len(vmo) {
        Some(capacity) if capacity >= len as usize => {}
        Some(_) => return status(VfsError::TooBig),
        None => return status(VfsError::Invalid),
    }
    store.write_content_from(&path, u64::from(len), &mut |at, out| {
        nexus_abi::vmo_read(vmo, at as usize, out).is_ok()
    })
}

fn status(err: VfsError) -> Vec<u8> {
    fileops::encode_status_reply(err.code())
}
