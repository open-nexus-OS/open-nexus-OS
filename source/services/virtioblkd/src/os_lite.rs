// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]

//! CONTEXT: virtioblkd — THE single virtio-blk owner (ADR-0044 end state,
//! TASK-0315): opens the ONE GPT disk, parses the table once (RO,
//! CRC-validated by `storage::gpt`), and serves partition-scoped blockproto
//! requests over IPC. Access is deny-by-default on the KERNEL-ATTRIBUTED
//! sender id: statefsd → `state` (rw), the nxfs owner (vfsd) → `data`
//! (rw); everyone else is `STATUS_DENIED` — the cross-partition deny is a
//! gated selftest. The service's own @reply inbox doubles as the driver's
//! IRQ notify endpoint (this service makes no outbound calls, so the inbox
//! is exclusively the interrupt channel) — completion waits BLOCK on the
//! device interrupt (`blk: irq completion on`) instead of yield-polling.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: blockproto codec host tests + QEMU ladder
//!   (`virtioblkd: gpt ok`, statefs/nxfs persistence over IPC, keep-blk
//!   double boot, cross-partition deny).
//! ADR: docs/adr/0044-single-blk-device-gpt-partitions-block-layer.md

use nexus_ipc::{KernelServer, Server as _, Wait};
use storage::blockproto::{self, BlockRequest, MAX_BLOCKS_PER_REQ, SECTOR_SIZE};
use storage::gpt::{self, Partition};
use storage::virtio_blk::VirtioBlkDevice;
use storage::BlockDevice;

/// Deterministic MMIO cap slot owned by init distribution.
const MMIO_CAP_SLOT: u32 = 48;

fn emit(msg: &str) {
    let _ = nexus_abi::debug_println(msg);
}

/// Resolved partition table entry: selector → absolute sector window.
#[derive(Clone, Copy)]
struct PartWindow {
    first_lba: u64,
    sectors: u64,
}

struct Served {
    dev: VirtioBlkDevice,
    parts: [Option<PartWindow>; blockproto::PART_COUNT as usize],
}

impl Served {
    fn window(&self, part: u8) -> Option<PartWindow> {
        self.parts.get(part as usize).copied().flatten()
    }
}

/// Per-sender partition grants (kernel-attributed identity; ADR-0044:
/// least privilege, one owner per store).
struct Gates {
    sid_statefsd: u64,
    sid_vfsd: u64,
    sid_bootctld: u64,
    sid_updated: u64,
    /// TASK-0321 (RFC-0089 §12.5): the system-volume verifier/reader.
    sid_bundlemgrd: u64,
}

impl Gates {
    /// Op-aware matrix (RFC-0089 §12.5): READ/INFO vs WRITE/SYNC can have
    /// different holders — the system volumes are read by bundlemgrd (and
    /// updated, for unchanged-bundle reuse) but written only by updated.
    fn allowed(&self, sender: u64, part: u8, op: u8) -> bool {
        let read_only = matches!(op, blockproto::OP_READ | blockproto::OP_INFO);
        match part {
            blockproto::PART_STATE => sender == self.sid_statefsd,
            blockproto::PART_DATA => sender == self.sid_vfsd,
            // TASK-0036-B: the BSB runtime writer is bootctld and ONLY
            // bootctld (ADR-0058; the loader writes pre-OS, nx image at
            // the factory).
            blockproto::PART_BSB => sender == self.sid_bootctld,
            // TASK-0179 (RFC-0089 §2): slot partitions are written only by
            // updated (the engine itself refuses the ACTIVE slot; this
            // gate scopes the sender, the engine scopes the slot).
            blockproto::PART_BOOT_A | blockproto::PART_BOOT_B => sender == self.sid_updated,
            // TASK-0321 (RFC-0089 §12.5): system volumes — bundlemgrd verifies
            // + serves (READ), updated assembles the INACTIVE one (WRITE; the
            // engine scopes the slot) and reads the ACTIVE one for reuse.
            blockproto::PART_SYSTEM_A | blockproto::PART_SYSTEM_B => {
                sender == self.sid_updated || (read_only && sender == self.sid_bundlemgrd)
            }
            // Anything else: deny-by-default.
            _ => false,
        }
    }
}

/// Opens the device, parses the GPT and maps the RFC-0089 selectors.
fn attach() -> Option<Served> {
    // Bounded wait for the init MMIO grant (grant lands after spawn).
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(5_000_000_000);
    let dev = loop {
        let mut q = nexus_abi::CapQuery { kind_tag: 0, reserved: 0, base: 0, len: 0 };
        if nexus_abi::cap_query(MMIO_CAP_SLOT, &mut q).is_ok() && q.kind_tag == 2 {
            match VirtioBlkDevice::new(MMIO_CAP_SLOT) {
                Ok(dev) => break dev,
                Err(_) => {
                    emit("virtioblkd: device open FAIL");
                    return None;
                }
            }
        }
        if nexus_abi::nsec().unwrap_or(u64::MAX) >= deadline {
            emit("virtioblkd: mmio grant timeout");
            return None;
        }
        let _ = nexus_abi::yield_();
    };

    let table = match gpt::parse_gpt(&dev) {
        Ok(table) => table,
        Err(_) => {
            emit("virtioblkd: gpt parse FAIL");
            return None;
        }
    };
    let mut parts: [Option<PartWindow>; blockproto::PART_COUNT as usize] =
        [None; blockproto::PART_COUNT as usize];
    let mut found = 0usize;
    for sel in 0..blockproto::PART_COUNT {
        let Some(name) = blockproto::part_layout_name(sel) else { continue };
        let hit: Option<&Partition> = table.iter().find(|p| p.name == name);
        if let Some(p) = hit {
            parts[sel as usize] =
                Some(PartWindow { first_lba: p.first_lba, sectors: p.last_lba - p.first_lba + 1 });
            found += 1;
        }
    }
    emit_gpt_ok(found);
    Some(Served { dev, parts })
}

/// `virtioblkd: gpt ok (parts=N)` — bounded formatting (N ≤ 9).
fn emit_gpt_ok(count: usize) {
    let mut line = *b"virtioblkd: gpt ok (parts=0)";
    let idx = line.len() - 2;
    line[idx] = b'0' + (count.min(9) as u8);
    if let Ok(msg) = core::str::from_utf8(&line) {
        emit(msg);
    }
}

/// Serves one decoded request into `out` (ZERO allocation — the bump heap
/// never frees; per-request `Vec`s killed this service in bring-up).
/// Returns the reply length.
fn serve(served: &mut Served, gates: &Gates, sender: u64, frame: &[u8], out: &mut [u8]) -> usize {
    let Some((nonce, req)) = blockproto::decode_request(frame) else {
        // No decodable op/nonce: answer with op 0 so the caller's decode
        // fails closed instead of hanging until its deadline.
        return blockproto::write_rsp_header(out, 0, 0, blockproto::STATUS_MALFORMED);
    };
    let (op, part) = match &req {
        BlockRequest::Info { part } => (blockproto::OP_INFO, *part),
        BlockRequest::Read { part, .. } => (blockproto::OP_READ, *part),
        BlockRequest::Write { part, .. } => (blockproto::OP_WRITE, *part),
        BlockRequest::Sync { part } => (blockproto::OP_SYNC, *part),
    };
    let Some(window) = served.window(part) else {
        return blockproto::write_rsp_header(out, op, nonce, blockproto::STATUS_UNKNOWN_PART);
    };
    if !gates.allowed(sender, part, op) {
        emit("virtioblkd: denied (partition gate)");
        return blockproto::write_rsp_header(out, op, nonce, blockproto::STATUS_DENIED);
    }
    match req {
        BlockRequest::Info { .. } => {
            blockproto::encode_info_reply_into(out, nonce, SECTOR_SIZE as u32, window.sectors)
        }
        BlockRequest::Read { lba, count, .. } => {
            if lba >= window.sectors || count as u64 > window.sectors - lba {
                return blockproto::write_rsp_header(
                    out,
                    op,
                    nonce,
                    blockproto::STATUS_OUT_OF_RANGE,
                );
            }
            let len = count as usize * SECTOR_SIZE;
            let base = blockproto::write_rsp_header(out, op, nonce, blockproto::STATUS_OK);
            match served.dev.read_blocks(window.first_lba + lba, &mut out[base..base + len]) {
                Ok(()) => base + len,
                Err(_) => blockproto::write_rsp_header(out, op, nonce, blockproto::STATUS_IO),
            }
        }
        BlockRequest::Write { lba, data, .. } => {
            let sectors = (data.len() / SECTOR_SIZE) as u64;
            if lba >= window.sectors || sectors > window.sectors - lba {
                return blockproto::write_rsp_header(
                    out,
                    op,
                    nonce,
                    blockproto::STATUS_OUT_OF_RANGE,
                );
            }
            match served.dev.write_blocks(window.first_lba + lba, data) {
                Ok(()) => blockproto::write_rsp_header(out, op, nonce, blockproto::STATUS_OK),
                Err(_) => blockproto::write_rsp_header(out, op, nonce, blockproto::STATUS_IO),
            }
        }
        BlockRequest::Sync { .. } => match served.dev.sync() {
            Ok(()) => blockproto::write_rsp_header(out, op, nonce, blockproto::STATUS_OK),
            Err(_) => blockproto::write_rsp_header(out, op, nonce, blockproto::STATUS_IO),
        },
    }
}

pub fn os_entry() -> Result<(), nexus_abi::AbiError> {
    nexus_abi::service_verdict_arm();

    // Server endpoint: the declarative arm provisions slots 3/4; resolve
    // via the responder with the deterministic fallback (logd pattern).
    let server = match crate::route_os::route_virtioblkd_blocking() {
        Some(server) => server,
        None => {
            emit("virtioblkd: route fallback");
            match KernelServer::new_with_slots(3, 4) {
                Ok(server) => server,
                Err(_) => {
                    emit("virtioblkd: server endpoint FAIL");
                    return Err(nexus_abi::AbiError::Unsupported);
                }
            }
        }
    };

    let mut served = attach();
    if served.is_none() {
        // Honest degrade: stay up and answer IO errors — a parked stub
        // would wedge every storage client behind their own deadlines.
        emit("virtioblkd: serving without device (degraded)");
    }

    // IRQ completion (TASK-0314 machinery + TASK-0315 provisioning): init
    // wires a DEDICATED notify endpoint at the fixed slot 0xF1 during
    // spawn-time distribution — no route round-trip, no shared traffic.
    const IRQ_NOTIFY_SLOT: u32 = 0xF1;
    if let Some(s) = served.as_mut() {
        if s.dev.bind_irq_endpoint(IRQ_NOTIFY_SLOT) {
            emit("virtioblkd: irq endpoint bound");
        }
    }

    emit("virtioblkd: ready");
    nexus_abi::service_verdict_flush("virtioblkd");

    let gates = Gates {
        sid_statefsd: nexus_abi::service_id_from_name(b"statefsd"),
        sid_vfsd: nexus_abi::service_id_from_name(b"vfsd"),
        sid_bootctld: nexus_abi::service_id_from_name(b"bootctld"),
        sid_updated: nexus_abi::service_id_from_name(b"updated"),
        sid_bundlemgrd: nexus_abi::service_id_from_name(b"bundlemgrd"),
    };

    let mut breaker = nexus_ipc::resilience::CircuitBreaker::new(64, 3);
    let mut inbuf = [0u8; blockproto::HDR_LEN + 10 + MAX_BLOCKS_PER_REQ as usize * SECTOR_SIZE];
    let mut outbuf = [0u8; blockproto::HDR_LEN + 1 + MAX_BLOCKS_PER_REQ as usize * SECTOR_SIZE];
    loop {
        match server.recv_request_with_meta_into(Wait::Blocking, &mut inbuf) {
            Ok((n, sender, reply)) => {
                breaker.on_success();
                let rn = match served.as_mut() {
                    Some(s) => serve(s, &gates, sender, &inbuf[..n], &mut outbuf),
                    None => {
                        // Device never attached: every op is an IO error
                        // (correlated when decodable).
                        match blockproto::decode_request(&inbuf[..n]) {
                            Some((nonce, req)) => {
                                let op = match req {
                                    BlockRequest::Info { .. } => blockproto::OP_INFO,
                                    BlockRequest::Read { .. } => blockproto::OP_READ,
                                    BlockRequest::Write { .. } => blockproto::OP_WRITE,
                                    BlockRequest::Sync { .. } => blockproto::OP_SYNC,
                                };
                                blockproto::write_rsp_header(
                                    &mut outbuf,
                                    op,
                                    nonce,
                                    blockproto::STATUS_IO,
                                )
                            }
                            None => blockproto::write_rsp_header(
                                &mut outbuf,
                                0,
                                0,
                                blockproto::STATUS_MALFORMED,
                            ),
                        }
                    }
                };
                let rsp = &outbuf[..rn];
                if let Some(reply) = reply {
                    if reply.reply_and_close(rsp).is_err() {
                        emit("virtioblkd: reply send fail");
                    }
                } else if server.send(rsp, Wait::NonBlocking).is_err() {
                    emit("virtioblkd: rsp send fail (dropping)");
                }
            }
            Err(nexus_ipc::IpcError::WouldBlock) | Err(nexus_ipc::IpcError::Timeout) => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => {
                let (should_log, verdict) = breaker.on_error();
                if should_log {
                    emit("virtioblkd: transient ipc error (continuing)");
                }
                match verdict {
                    nexus_ipc::resilience::BreakerVerdict::Continue => {
                        let _ = nexus_abi::yield_();
                    }
                    nexus_ipc::resilience::BreakerVerdict::EndpointDefect => {
                        emit("virtioblkd: endpoint defect (consecutive error limit)");
                        return Err(nexus_abi::AbiError::Unsupported);
                    }
                }
            }
        }
    }
}
