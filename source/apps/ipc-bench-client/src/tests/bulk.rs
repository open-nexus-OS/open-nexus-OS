// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Test C: Bulk Transfer (Copy-Chunking vs VMO)

extern crate alloc;
use alloc::vec::Vec;
use nexus_abi::{ipc_recv_v1, ipc_send_v1, nsec, vmo_create, vmo_write, MsgHeader};

const OP_BULK_META: u16 = 2;

#[derive(Debug, Clone)]
pub struct BulkResult {
    pub method: &'static str,
    pub size: usize,
    pub duration_ns: u64,
    pub throughput_mbps: f64,
}

pub fn run_bulk_tests() -> Vec<BulkResult> {
    let sizes = [65536, 1048576]; // 64KB, 1MB (reduced for quick results)
    let mut results = Vec::new();

    for &size in &sizes {
        // Copy-chunking baseline
        let copy_result = bulk_copy_chunking(size);
        results.push(copy_result);

        // VMO zero-copy
        let vmo_result = bulk_vmo(size);
        results.push(vmo_result);
    }

    results
}

fn bulk_copy_chunking(total_size: usize) -> BulkResult {
    let (ep_a, _ep_b) = super::get_endpoints();
    let send_slot = ep_a;
    let recv_slot = ep_a;
    let chunk_size = 8192;
    let num_chunks = total_size / chunk_size;

    // Pre-allocate data
    let mut data = Vec::with_capacity(chunk_size);
    data.resize(chunk_size, 0xCC);

    let start = nsec().unwrap_or(0);
    use nexus_abi::IPC_SYS_NONBLOCK;

    // Send chunks (non-blocking)
    for _ in 0..num_chunks {
        let hdr = MsgHeader::new(0, 0, OP_BULK_META, 0, chunk_size as u32);
        let _ = ipc_send_v1(send_slot, &hdr, &data, IPC_SYS_NONBLOCK, 0);
    }

    // Receive all chunks back (loopback)
    let mut ack_buf = [0u8; 8192];
    for _ in 0..num_chunks {
        let mut ack_hdr = MsgHeader::new(0, 0, 0, 0, 0);
        let _ = ipc_recv_v1(recv_slot, &mut ack_hdr, &mut ack_buf, 0, 0);
    }

    let end = nsec().unwrap_or(0);
    let duration = end.saturating_sub(start);
    let throughput = (total_size as f64) / ((duration as f64) / 1_000_000_000.0) / 1_000_000.0;

    BulkResult {
        method: "copy_chunking",
        size: total_size,
        duration_ns: duration,
        throughput_mbps: throughput,
    }
}

fn bulk_vmo(total_size: usize) -> BulkResult {
    let (ep_a, _ep_b) = super::get_endpoints();
    let send_slot = ep_a;
    let recv_slot = ep_a;

    // Create VMO
    let vmo_slot = match vmo_create(total_size) {
        Ok(slot) => slot,
        Err(_) => return BulkResult {
            method: "vmo",
            size: total_size,
            duration_ns: 0,
            throughput_mbps: 0.0,
        },
    };

    // Fill VMO in 64KB chunks
    let mut chunk = Vec::with_capacity(65536);
    chunk.resize(65536, 0xDD);

    for offset in (0..total_size).step_by(65536) {
        let write_len = core::cmp::min(65536, total_size - offset);
        let _ = vmo_write(vmo_slot, offset, &chunk[..write_len]);
    }

    // Send metadata (VMO handle would be transferred via cap_transfer in real impl)
    // For now: send metadata frame with handle slot
    let start = nsec().unwrap_or(0);

    let mut meta_buf = [0u8; 32];
    // Encode: vmo_handle (u32), offset (u64), len (u64)
    meta_buf[0..4].copy_from_slice(&(vmo_slot as u32).to_le_bytes());
    meta_buf[8..16].copy_from_slice(&(0u64).to_le_bytes()); // offset
    meta_buf[16..24].copy_from_slice(&(total_size as u64).to_le_bytes());

    use nexus_abi::IPC_SYS_NONBLOCK;

    let meta_hdr = MsgHeader::new(0, 0, OP_BULK_META, 0, 32);
    let _ = ipc_send_v1(send_slot, &meta_hdr, &meta_buf, IPC_SYS_NONBLOCK, 0);

    // Receive metadata back (loopback)
    let mut ack_buf = [0u8; 32];
    let mut ack_hdr = MsgHeader::new(0, 0, 0, 0, 0);
    let _ = ipc_recv_v1(recv_slot, &mut ack_hdr, &mut ack_buf, 0, 0);

    let end = nsec().unwrap_or(0);
    let duration = end.saturating_sub(start);
    let throughput = (total_size as f64) / ((duration as f64) / 1_000_000_000.0) / 1_000_000.0;

    BulkResult {
        method: "vmo",
        size: total_size,
        duration_ns: duration,
        throughput_mbps: throughput,
    }
}
