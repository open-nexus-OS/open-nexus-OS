// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: `RemoteBlockDevice` — the partition-scoped block client
//! (ADR-0044/TASK-0315): statefsd and nxfsd speak `BlockDevice` against
//! virtioblkd over the blockproto wire instead of owning device MMIO.
//! Transport = the canonical service-client shape (statefs client
//! lineage): request on the target's SEND slot with a CAP_MOVE reply
//! clone, reply correlated by magic + nonce on the caller's SHARED reply
//! inbox (stranger frames are drained, never misparsed). Bounded
//! everything: ≤ MAX_BLOCKS_PER_REQ sectors per message, 2 s per-op
//! deadline, fail-closed on any malformed reply.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: codec host-proven in `blockproto`; the transport is
//!   proven by the QEMU persistence/`/data` ladder (statefs + nxfs over
//!   IPC, keep-blk double boot).
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use core::sync::atomic::{AtomicU32, Ordering};

use crate::blockproto::{self, MAX_BLOCKS_PER_REQ, SECTOR_SIZE};
use crate::{BlockDevice, BlockError};

/// Default per-op deadline (bounded; wait loops self-terminate).
const OP_DEADLINE_NS: u64 = 2_000_000_000;
/// Fixed request/response buffer sizes (12 sectors + framing).
const REQ_BUF: usize = blockproto::HDR_LEN + 9 + MAX_BLOCKS_PER_REQ as usize * SECTOR_SIZE;
const RSP_BUF: usize = blockproto::HDR_LEN + 1 + MAX_BLOCKS_PER_REQ as usize * SECTOR_SIZE;

/// Process-wide nonce for reply correlation (RFC-0019 discipline).
static NONCE: AtomicU32 = AtomicU32::new(1);

/// Partition-scoped remote block device (one selector per instance).
pub struct RemoteBlockDevice {
    /// SEND slot to virtioblkd's request endpoint.
    send_slot: u32,
    /// The caller's reply inbox (SEND clone travels per request).
    reply_send_slot: u32,
    reply_recv_slot: u32,
    part: u8,
    block_count: u64,
}

impl RemoteBlockDevice {
    /// Opens the partition: one INFO round-trip proves the server is up,
    /// the partition exists and the caller is allowed to see it. `None`
    /// keeps the caller in its bounded retry window (virtioblkd may come
    /// up after the client).
    pub fn open(
        send_slot: u32,
        reply_send_slot: u32,
        reply_recv_slot: u32,
        part: u8,
    ) -> Option<Self> {
        Self::open_with_deadline(send_slot, reply_send_slot, reply_recv_slot, part, OP_DEADLINE_NS)
    }

    /// `open` with a caller-chosen INFO deadline: attach probes inside a
    /// bounded retry window (statefsd pristine upgrade) must stay cheap
    /// while virtioblkd is still bringing the device up.
    pub fn open_with_deadline(
        send_slot: u32,
        reply_send_slot: u32,
        reply_recv_slot: u32,
        part: u8,
        deadline_budget_ns: u64,
    ) -> Option<Self> {
        let mut dev = Self { send_slot, reply_send_slot, reply_recv_slot, part, block_count: 0 };
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let mut req = [0u8; REQ_BUF];
        let n = blockproto::encode_info_into(&mut req, nonce, part);
        let mut rsp = [0u8; RSP_BUF];
        let rn = dev.round_trip_deadline(&req[..n], &mut rsp, deadline_budget_ns).ok()?;
        let (block_size, block_count) = blockproto::decode_info_reply(nonce, &rsp[..rn])?;
        if block_size as usize != SECTOR_SIZE || block_count == 0 {
            return None;
        }
        dev.block_count = block_count;
        Some(dev)
    }

    /// One bounded request/reply round trip into the caller's buffer
    /// (ZERO allocation — bump-allocator services never free; shared-inbox
    /// correlation is the caller's via the nonce inside `frame`).
    fn round_trip(&self, frame: &[u8], rsp: &mut [u8]) -> Result<usize, BlockError> {
        self.round_trip_deadline(frame, rsp, OP_DEADLINE_NS)
    }

    fn round_trip_deadline(
        &self,
        frame: &[u8],
        rsp: &mut [u8],
        budget_ns: u64,
    ) -> Result<usize, BlockError> {
        let moved = nexus_abi::cap_clone(self.reply_send_slot).map_err(|_| BlockError::IoError)?;
        let hdr = nexus_abi::MsgHeader::new(
            moved,
            0,
            0,
            nexus_abi::ipc_hdr::CAP_MOVE,
            frame.len() as u32,
        );
        let start = nexus_abi::nsec().map_err(|_| BlockError::IoError)?;
        let deadline = start.saturating_add(budget_ns);

        let mut i: usize = 0;
        loop {
            match nexus_abi::ipc_send_v1(
                self.send_slot,
                &hdr,
                frame,
                nexus_abi::IPC_SYS_NONBLOCK,
                0,
            ) {
                Ok(_) => break,
                Err(nexus_abi::IpcError::QueueFull) => {
                    if (i & 0x7f) == 0 && nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
                        let _ = nexus_abi::cap_close(moved);
                        return Err(BlockError::IoError);
                    }
                    let _ = nexus_abi::yield_();
                }
                Err(_) => {
                    let _ = nexus_abi::cap_close(moved);
                    return Err(BlockError::IoError);
                }
            }
            i = i.wrapping_add(1);
        }

        let mut rh = nexus_abi::MsgHeader::new(0, 0, 0, 0, 0);
        let mut j: usize = 0;
        loop {
            if (j & 0x7f) == 0 && nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
                return Err(BlockError::IoError);
            }
            j = j.wrapping_add(1);
            match nexus_abi::ipc_recv_v1(
                self.reply_recv_slot,
                &mut rh,
                rsp,
                nexus_abi::IPC_SYS_NONBLOCK | nexus_abi::IPC_SYS_TRUNCATE,
                0,
            ) {
                Ok(n) => {
                    let n = core::cmp::min(n as usize, rsp.len());
                    // Ours iff blockproto magic — statefs/policyd replies on
                    // the shared inbox are someone else's; the nonce check
                    // happens in the caller's decode.
                    if n >= blockproto::HDR_LEN + 1
                        && rsp[0] == blockproto::MAGIC0
                        && rsp[1] == blockproto::MAGIC1
                    {
                        return Ok(n);
                    }
                }
                Err(nexus_abi::IpcError::QueueEmpty) => {
                    let _ = nexus_abi::yield_();
                }
                Err(_) => return Err(BlockError::IoError),
            }
        }
    }

    /// TASK-0321 P4b: arms a clone of `vmo` at virtioblkd for this sender
    /// (no reply; the queue is FIFO so the following READ_VMO sees it).
    /// The caller keeps its own handle.
    pub fn arm_vmo(&self, vmo: u32) -> Result<(), BlockError> {
        let moved = nexus_abi::cap_clone(vmo).map_err(|_| BlockError::IoError)?;
        let mut req = [0u8; 16];
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let n = blockproto::encode_arm_vmo_into(&mut req, nonce, self.part);
        let hdr = nexus_abi::MsgHeader::new(moved, 0, 0, nexus_abi::ipc_hdr::CAP_MOVE, n as u32);
        let deadline =
            nexus_abi::nsec().map_err(|_| BlockError::IoError)?.saturating_add(OP_DEADLINE_NS);
        loop {
            match nexus_abi::ipc_send_v1(
                self.send_slot,
                &hdr,
                &req[..n],
                nexus_abi::IPC_SYS_NONBLOCK,
                0,
            ) {
                Ok(_) => return Ok(()),
                Err(nexus_abi::IpcError::QueueFull) => {
                    if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
                        let _ = nexus_abi::cap_close(moved);
                        return Err(BlockError::IoError);
                    }
                    let _ = nexus_abi::yield_();
                }
                Err(_) => {
                    let _ = nexus_abi::cap_close(moved);
                    return Err(BlockError::IoError);
                }
            }
        }
    }

    /// TASK-0321 P4b: copies `len` partition bytes from `byte_off` into the
    /// armed VMO at `vmo_off` — ONE round trip for a whole bundle window
    /// (the driver streams device runs, no IPC per run). Deadline scales
    /// with the transfer (1 s per MiB on top of the base budget).
    pub fn read_into_vmo(&self, byte_off: u64, len: u64, vmo_off: u64) -> Result<(), BlockError> {
        if len == 0 || len > blockproto::MAX_VMO_READ_BYTES {
            return Err(BlockError::OutOfRange);
        }
        let end = byte_off.checked_add(len).ok_or(BlockError::OutOfRange)?;
        if end > self.block_count.saturating_mul(SECTOR_SIZE as u64) {
            return Err(BlockError::OutOfRange);
        }
        let mut req = [0u8; 40];
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let n =
            blockproto::encode_read_vmo_into(&mut req, nonce, self.part, byte_off, len, vmo_off);
        let mut rsp = [0u8; blockproto::HDR_LEN + 1];
        let budget =
            OP_DEADLINE_NS.saturating_add(len.div_ceil(1 << 20).saturating_mul(1_000_000_000));
        let rn = self.round_trip_deadline(&req[..n], &mut rsp, budget)?;
        match blockproto::decode_status(blockproto::OP_READ_VMO, nonce, &rsp[..rn]) {
            Some(blockproto::STATUS_OK) => Ok(()),
            Some(blockproto::STATUS_OUT_OF_RANGE) => Err(BlockError::OutOfRange),
            _ => Err(BlockError::IoError),
        }
    }

    /// TASK-0321 P4b: closes the armed VMO at the driver.
    pub fn release_vmo(&self) -> Result<(), BlockError> {
        let mut req = [0u8; 16];
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let n = blockproto::encode_release_vmo_into(&mut req, nonce, self.part);
        let mut rsp = [0u8; blockproto::HDR_LEN + 1];
        let rn = self.round_trip(&req[..n], &mut rsp)?;
        match blockproto::decode_status(blockproto::OP_RELEASE_VMO, nonce, &rsp[..rn]) {
            Some(blockproto::STATUS_OK) => Ok(()),
            _ => Err(BlockError::IoError),
        }
    }

    fn io_run(&self, op_write: bool, first: u64, buf_len: usize) -> Result<(), BlockError> {
        #[allow(unknown_lints, clippy::manual_is_multiple_of)]
        let misaligned = buf_len == 0 || buf_len % SECTOR_SIZE != 0;
        if misaligned {
            return Err(BlockError::OutOfRange);
        }
        let sectors = (buf_len / SECTOR_SIZE) as u64;
        if first >= self.block_count || sectors > self.block_count - first {
            return Err(BlockError::OutOfRange);
        }
        let _ = op_write;
        Ok(())
    }
}

impl BlockDevice for RemoteBlockDevice {
    fn block_size(&self) -> usize {
        SECTOR_SIZE
    }

    fn block_count(&self) -> u64 {
        self.block_count
    }

    fn read_block(&self, block_idx: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        if buf.len() < SECTOR_SIZE {
            return Err(BlockError::IoError);
        }
        self.read_blocks(block_idx, &mut buf[..SECTOR_SIZE])
    }

    fn write_block(&mut self, block_idx: u64, buf: &[u8]) -> Result<(), BlockError> {
        if buf.len() < SECTOR_SIZE {
            return Err(BlockError::IoError);
        }
        self.write_blocks(block_idx, &buf[..SECTOR_SIZE])
    }

    fn read_blocks(&self, first_block: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        self.io_run(false, first_block, buf.len())?;
        let mut req = [0u8; REQ_BUF];
        let mut rsp = [0u8; RSP_BUF];
        let mut next = first_block;
        for chunk in buf.chunks_mut(MAX_BLOCKS_PER_REQ as usize * SECTOR_SIZE) {
            let count = (chunk.len() / SECTOR_SIZE) as u16;
            let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
            let n = blockproto::encode_read_into(&mut req, nonce, self.part, next, count);
            let rn = self.round_trip(&req[..n], &mut rsp)?;
            let data = blockproto::decode_read_reply(nonce, &rsp[..rn], count)
                .ok_or(BlockError::IoError)?;
            chunk.copy_from_slice(data);
            next += count as u64;
        }
        Ok(())
    }

    fn write_blocks(&mut self, first_block: u64, buf: &[u8]) -> Result<(), BlockError> {
        self.io_run(true, first_block, buf.len())?;
        let mut req = [0u8; REQ_BUF];
        let mut rsp = [0u8; RSP_BUF];
        let mut next = first_block;
        for chunk in buf.chunks(MAX_BLOCKS_PER_REQ as usize * SECTOR_SIZE) {
            let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
            let n = blockproto::encode_write_into(&mut req, nonce, self.part, next, chunk);
            let rn = self.round_trip(&req[..n], &mut rsp)?;
            match blockproto::decode_status(blockproto::OP_WRITE, nonce, &rsp[..rn]) {
                Some(blockproto::STATUS_OK) => {}
                Some(blockproto::STATUS_OUT_OF_RANGE) => return Err(BlockError::OutOfRange),
                _ => return Err(BlockError::IoError),
            }
            next += (chunk.len() / SECTOR_SIZE) as u64;
        }
        Ok(())
    }

    fn sync(&mut self) -> Result<(), BlockError> {
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let mut req = [0u8; REQ_BUF];
        let mut rsp = [0u8; RSP_BUF];
        let n = blockproto::encode_sync_into(&mut req, nonce, self.part);
        let rn = self.round_trip(&req[..n], &mut rsp)?;
        match blockproto::decode_status(blockproto::OP_SYNC, nonce, &rsp[..rn]) {
            Some(blockproto::STATUS_OK) => Ok(()),
            _ => Err(BlockError::IoError),
        }
    }
}
