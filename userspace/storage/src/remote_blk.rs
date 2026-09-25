// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", feature = "os-lite"))]

//! CONTEXT: `RemoteBlockDevice` — the partition-scoped block client
//! (ADR-0044/TASK-0315): statefsd and nxfsd speak `BlockDevice` against
//! blkd over the blockproto wire instead of owning device MMIO.
//! Transport = the canonical service-client shape (statefs client
//! lineage): request on the target's SEND slot with a CAP_MOVE reply
//! clone, reply correlated by magic + nonce on the caller's SHARED reply
//! inbox (stranger frames are drained, never misparsed). Bounded
//! everything: ≤ MAX_BLOCKS_PER_REQ sectors per message, no clock on a request (it
//! ends on its reply or on the server's death, RFC-0093 §7), fail-closed on any
//! malformed reply.
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

/// Fixed request/response buffer sizes (12 sectors + framing).
const REQ_BUF: usize = blockproto::HDR_LEN + 9 + MAX_BLOCKS_PER_REQ as usize * SECTOR_SIZE;
const RSP_BUF: usize = blockproto::HDR_LEN + 1 + MAX_BLOCKS_PER_REQ as usize * SECTOR_SIZE;

/// Process-wide nonce for reply correlation (RFC-0019 discipline).
static NONCE: AtomicU32 = AtomicU32::new(1);

/// Partition-scoped remote block device (one selector per instance).
pub struct RemoteBlockDevice {
    /// SEND slot to blkd's request endpoint.
    send_slot: u32,
    /// The caller's reply inbox (SEND clone travels per request).
    reply_send_slot: u32,
    reply_recv_slot: u32,
    part: u8,
    block_count: u64,
}

impl RemoteBlockDevice {
    /// Opens the partition: ONE INFO round trip proves the server is up, the partition
    /// exists and the caller is allowed to see it — a clock-free wait (TASK-0324 P7-d):
    /// blkd's answer or its death. `None` = the partition is not there.
    pub fn open(
        send_slot: u32,
        reply_send_slot: u32,
        reply_recv_slot: u32,
        part: u8,
    ) -> Option<Self> {
        let mut dev = Self { send_slot, reply_send_slot, reply_recv_slot, part, block_count: 0 };
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let mut req = [0u8; REQ_BUF];
        let n = blockproto::encode_info_into(&mut req, nonce, part);
        let mut rsp = [0u8; RSP_BUF];
        let rn = dev.round_trip(&req[..n], &mut rsp).ok()?;
        let (block_size, block_count) = blockproto::decode_info_reply(nonce, &rsp[..rn])?;
        if block_size as usize != SECTOR_SIZE || block_count == 0 {
            return None;
        }
        dev.block_count = block_count;
        Some(dev)
    }

    /// One request/reply round trip into the caller's buffer (ZERO allocation —
    /// bump-allocator services never free). No clock (TASK-0324 P7-d): queue space, then the
    /// answer on the reply inbox, or blkd's death (EOF — it holds the moved cap).
    /// Shared-inbox correlation is the caller's via the nonce inside `frame`; a foreign
    /// frame (another op's late reply) is dropped and the wait resumes.
    fn round_trip(&self, frame: &[u8], rsp: &mut [u8]) -> Result<usize, BlockError> {
        nexus_ipc::exchange::call_matching(
            self.send_slot,
            nexus_ipc::SlotPair::new(self.reply_send_slot, self.reply_recv_slot),
            frame,
            rsp,
            |r| {
                (r.len() >= blockproto::HDR_LEN
                    && r[0] == blockproto::MAGIC0
                    && r[1] == blockproto::MAGIC1)
                    .then(|| r.len())
            },
        )
        .map_err(|_| BlockError::IoError)
    }

    /// TASK-0321 P4b: arms a clone of `vmo` at blkd for this sender
    /// (no reply; the queue is FIFO so the following READ_VMO sees it).
    /// The caller keeps its own handle.
    pub fn arm_vmo(&self, vmo: u32) -> Result<(), BlockError> {
        let moved = nexus_abi::cap_clone(vmo).map_err(|_| BlockError::IoError)?;
        let mut req = [0u8; 16];
        let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
        let n = blockproto::encode_arm_vmo_into(&mut req, nonce, self.part);
        // The moved cap is the destination VMO — data, not a reply inbox. No clock
        // (TASK-0324 P7-d): queue space or blkd's death.
        if nexus_ipc::exchange::send_with_cap(self.send_slot, &req[..n], moved).is_err() {
            let _ = nexus_abi::cap_close(moved);
            return Err(BlockError::IoError);
        }
        Ok(())
    }

    /// TASK-0321 P4b: copies `len` partition bytes from `byte_off` into the
    /// armed VMO at `vmo_off` — ONE round trip for a whole bundle window
    /// (the driver streams device runs, no IPC per run).
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
        let rn = self.round_trip(&req[..n], &mut rsp)?;
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
