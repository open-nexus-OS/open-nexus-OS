// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Marker reader — polls logd for expected UART markers.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder
//!
//! Pure observer: never initiates service IPC beyond reading from logd.

// RFC-0061 M4 pure-observer toolkit (marker-reader): declared observer API surface,
// kept per ADR-0027 until the observer ladder wires it in — module-scoped
// allow, not crate-level (repo rule).
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use nexus_abi::yield_;
use nexus_ipc::KernelClient;

/// Wait for a specific marker string to appear in logd output.
///
/// Returns `true` if the marker was found within the deadline.
pub(crate) fn wait_for_marker(logd: &KernelClient, marker: &[u8], deadline_ns: u64) -> bool {
    let start = nexus_abi::nsec().unwrap_or(0);
    let deadline = start.saturating_add(deadline_ns);
    loop {
        if logd_contains(logd, 0, marker) {
            return true;
        }
        let now = nexus_abi::nsec().unwrap_or(0);
        if now >= deadline {
            return false;
        }
        for _ in 0..32 {
            let _ = yield_();
        }
    }
}

/// Wait for a list of markers in sequence. Returns the index of the first missing marker,
/// or `None` if all were found.
pub(crate) fn wait_for_markers(
    logd: &KernelClient,
    markers: &[&[u8]],
    deadline_per_marker_ns: u64,
) -> Option<usize> {
    for (i, marker) in markers.iter().enumerate() {
        if !wait_for_marker(logd, marker, deadline_per_marker_ns) {
            return Some(i);
        }
    }
    None
}

/// Check if logd contains a specific byte sequence after a given offset.
fn logd_contains(logd: &KernelClient, offset: u64, pattern: &[u8]) -> bool {
    // Use logd's query protocol: send a QUERY frame, check response.
    // Minimal: try to read marker via a non-blocking probe.
    // Falls back to the existing logd_query_contains_since_paged pattern.
    let (send_slot, recv_slot) = logd.slots();

    // Build query frame: [L,G,1,OP_QUERY, offset:u64le, limit:u32le]
    let mut req = [0u8; 20];
    req[0] = b'L';
    req[1] = b'G';
    req[2] = 1; // version
    req[3] = 2; // OP_QUERY
    req[4..12].copy_from_slice(&offset.to_le_bytes());
    req[12..16].copy_from_slice(&4096u32.to_le_bytes()); // limit
    let _ = req; // suppress unused warning for now

    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 16);
    if nexus_abi::ipc_send_v1(send_slot, &hdr, &req[..16], nexus_abi::IPC_SYS_NONBLOCK, 0).is_err()
    {
        return false;
    }

    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 256];
    let n = match nexus_abi::ipc_recv_v1(
        recv_slot,
        &mut rh,
        &mut buf,
        nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
        0,
    ) {
        Ok(n) => n as usize,
        Err(_) => return false,
    };
    let n = n.min(buf.len());

    // Response: [L,G,1,OP_QUERY|0x80, status, payload...]
    if n < 5 || buf[0] != b'L' || buf[1] != b'G' || buf[2] != 1 {
        return false;
    }
    if buf[3] != (2 | 0x80) || buf[4] != 0 {
        return false;
    }

    // Search for pattern in response payload
    let payload = &buf[5..n];
    payload.windows(pattern.len()).any(|w| w == pattern)
}

/// Read all markers since a given timestamp. Returns the concatenated marker text.
pub(crate) fn read_markers_since(
    logd: &KernelClient,
    since_ns: u64,
    max_bytes: usize,
) -> Option<Vec<u8>> {
    let (send_slot, recv_slot) = logd.slots();
    let mut req = [0u8; 20];
    req[0] = b'L';
    req[1] = b'G';
    req[2] = 1;
    req[3] = 2; // OP_QUERY
    req[4..12].copy_from_slice(&since_ns.to_le_bytes());
    let limit = (max_bytes as u32).min(8192);
    req[12..16].copy_from_slice(&limit.to_le_bytes());

    let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, 16);
    if nexus_abi::ipc_send_v1(send_slot, &hdr, &req[..16], nexus_abi::IPC_SYS_NONBLOCK, 0).is_err()
    {
        return None;
    }

    let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
    let mut buf = [0u8; 256];
    let n = match nexus_abi::ipc_recv_v1(
        recv_slot,
        &mut rh,
        &mut buf,
        nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
        0,
    ) {
        Ok(n) => n as usize,
        Err(_) => return None,
    };
    let n = n.min(buf.len());
    if n < 5 || buf[0] != b'L' || buf[1] != b'G' || buf[4] != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(n - 5);
    out.extend_from_slice(&buf[5..n]);
    Some(out)
}
