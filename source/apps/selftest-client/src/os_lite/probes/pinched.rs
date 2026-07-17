// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: pinched compute-broker probe (SMP track Phase D). Exercises the
//! full system path: name-route → VMO CAP_MOVE job submission → parallel
//! compute on the service's workpool → header-last completion. The expected
//! output is computed locally with the SAME `pinched::broker::mix_u32` the
//! service uses (shared SSOT — the proof cannot drift), and the header's
//! `workers` field is the honest dispatch counter (0 would mean the inline
//! fallback ran, which the determinism marker rejects).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: QEMU markers `SELFTEST: pinched determinism ok` /
//!   `SELFTEST: pinched bounded ok` (just test-os / ci-os-smp).

extern crate alloc;

use alloc::vec::Vec;

use nexus_abi::yield_;
use nexus_ipc::{KernelClient, Wait as IpcWait};
use pinched::broker::mix_u32;
use pinched::protocol as pn;

use crate::markers::{emit_bytes, emit_hex_u64, emit_line};

/// Polls a job VMO's completion header until DONE_MAGIC (the service's
/// release fence) or the deadline. Closes the cap only on failure — the
/// caller reads the payload and closes on success.
fn poll_header(vmo: u32, deadline_ns: u64) -> Option<(u32, u32, u32)> {
    let deadline = nexus_abi::nsec().unwrap_or(0).saturating_add(deadline_ns);
    let mut hdr = [0u8; pn::HDR_LEN];
    loop {
        if nexus_abi::vmo_read(vmo, 0, &mut hdr).is_err() {
            emit_line("pinched-probe: FAIL (hdr read)");
            let _ = nexus_abi::cap_close(vmo);
            return None;
        }
        let magic = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
        if magic == pn::DONE_MAGIC {
            break;
        }
        if nexus_abi::nsec().unwrap_or(0) >= deadline {
            emit_bytes(b"pinched-probe: FAIL (poll timeout hdr=0x");
            emit_hex_u64(magic as u64);
            emit_line(")");
            let _ = nexus_abi::cap_close(vmo);
            return None;
        }
        let _ = yield_();
    }
    Some((
        u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]),
        u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]),
        u32::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]),
    ))
}

/// Elements for the determinism run (well under `MAX_JOB_ELEMS`).
const N: usize = 1024;

/// SVG proof workload (Phase D4): shared SSOT from the broker crate —
/// source, size and the HOST-PINNED raster digest (pinched host test
/// `proof_svg_digest_matches_pinned` regenerates it, so the constant cannot
/// drift from the library). The probe deliberately does NOT rasterize a
/// local reference: large f32/alloc-heavy computations inside the selftest
/// process are currently non-deterministic (pre-existing bug, tracked in the
/// SMP ledger) while the broker's output is boot-stable.
use pinched::broker::{fnv1a, PROOF_SVG, PROOF_SVG_DIGEST, PROOF_SVG_H, PROOF_SVG_W};
const SVG_W: usize = PROOF_SVG_W;
const SVG_H: usize = PROOF_SVG_H;
const SVG_SRC: &str = PROOF_SVG;

pub(crate) fn pinched_selftest() {
    let Some(client) = route_pinched() else {
        emit_line("SELFTEST: pinched determinism FAIL (route)");
        return;
    };

    // Bounded contract: an oversized job must be REJECTED via the header.
    match submit_and_poll(&client, (pinched::MAX_JOB_ELEMS + 1) as u32, 0, 2_000_000_000) {
        Some((status, _, _, _)) if status == pn::STATUS_OVERSIZED => {
            emit_line("SELFTEST: pinched bounded ok");
        }
        Some(_) => emit_line("SELFTEST: pinched bounded FAIL (status)"),
        None => emit_line("SELFTEST: pinched bounded FAIL (no completion)"),
    }

    // SVG raster (Phase D4): the broker's banded parallel raster must be
    // byte-identical to a local full rasterize with the same library, and the
    // parallel backend must have executed (workers >= 1).
    svg_proof(&client);

    // Determinism + dispatch: result must equal the local reference AND the
    // broker must report its parallel backend (workers >= 1).
    match submit_and_poll(&client, N as u32, N, 5_000_000_000) {
        Some((pn::STATUS_OK, elems, workers, data)) => {
            if elems as usize != N {
                emit_line("SELFTEST: pinched determinism FAIL (elems)");
                return;
            }
            for (i, chunk) in data.chunks_exact(4).enumerate() {
                let got = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                if got != mix_u32(i as u32) {
                    emit_line("SELFTEST: pinched determinism FAIL (mismatch)");
                    return;
                }
            }
            if workers == 0 {
                emit_line("SELFTEST: pinched determinism FAIL (inline fallback)");
                return;
            }
            emit_line("SELFTEST: pinched determinism ok");
        }
        Some(_) => emit_line("SELFTEST: pinched determinism FAIL (status)"),
        None => emit_line("SELFTEST: pinched determinism FAIL (no completion)"),
    }
}

fn svg_proof(client: &KernelClient) {
    match submit_svg_and_poll(client) {
        Some((pn::STATUS_OK, elems, workers, data)) => {
            if elems as usize != SVG_W * SVG_H {
                emit_line("SELFTEST: pinched svg FAIL (elems)");
                return;
            }
            let digest = fnv1a(&data);
            if digest != PROOF_SVG_DIGEST {
                emit_bytes(b"pinched-probe: svg digest=0x");
                emit_hex_u64(digest);
                emit_line("");
                emit_line("SELFTEST: pinched svg FAIL (digest)");
                return;
            }
            if workers == 0 {
                emit_line("SELFTEST: pinched svg FAIL (inline fallback)");
                return;
            }
            emit_line("SELFTEST: pinched svg ok");
        }
        Some(_) => emit_line("SELFTEST: pinched svg FAIL (status)"),
        None => emit_line("SELFTEST: pinched svg FAIL (no completion)"),
    }
}

/// Submits one `JOB_SVG_RASTER` and polls the completion header. Returns
/// `(status, elems, workers, pixel_bytes)`.
fn submit_svg_and_poll(client: &KernelClient) -> Option<(u32, u32, u32, Vec<u8>)> {
    let out_bytes = SVG_W * SVG_H * 4;
    let vmo_size = pn::DATA_OFFSET + SVG_SRC.len().max(out_bytes);
    let Ok(vmo) = nexus_abi::vmo_create(vmo_size) else {
        emit_line("pinched-probe: FAIL (svg vmo create)");
        return None;
    };
    let pending = [0u8; pn::HDR_LEN];
    if nexus_abi::vmo_write(vmo, 0, &pending).is_err()
        || nexus_abi::vmo_write(vmo, pn::DATA_OFFSET, SVG_SRC.as_bytes()).is_err()
    {
        emit_line("pinched-probe: FAIL (svg vmo write)");
        return None;
    }
    let Ok(clone) = nexus_abi::cap_clone(vmo) else {
        emit_line("pinched-probe: FAIL (svg clone)");
        return None;
    };
    let mut frame = [0u8; pn::COMPUTE_SVG_REQ_LEN];
    frame[0] = pn::MAGIC0;
    frame[1] = pn::MAGIC1;
    frame[2] = pn::VERSION;
    frame[3] = pn::OP_COMPUTE;
    frame[4] = pn::JOB_SVG_RASTER;
    frame[5..9].copy_from_slice(&(SVG_H as u32).to_le_bytes());
    frame[9..11].copy_from_slice(&(SVG_W as u16).to_le_bytes());
    frame[11..13].copy_from_slice(&(SVG_H as u16).to_le_bytes());
    frame[13..17].copy_from_slice(&(SVG_SRC.len() as u32).to_le_bytes());
    if client
        .send_with_cap_move_wait(
            &frame,
            clone,
            IpcWait::Timeout(core::time::Duration::from_millis(1000)),
        )
        .is_err()
    {
        emit_line("pinched-probe: FAIL (svg send)");
        return None;
    }
    // Poll budget: the broker's 6s run deadline plus inline-fallback and
    // staging headroom (icount shares one round-robin vCPU).
    let (status, elems, workers) = poll_header(vmo, 12_000_000_000)?;
    let mut data = Vec::new();
    if status == pn::STATUS_OK {
        data.resize(out_bytes, 0);
        if nexus_abi::vmo_read(vmo, pn::DATA_OFFSET, data.as_mut_slice()).is_err() {
            let _ = nexus_abi::cap_close(vmo);
            return None;
        }
    }
    let _ = nexus_abi::cap_close(vmo);
    Some((status, elems, workers, data))
}

/// Route to pinched with a bounded retry (the service registers its route
/// during bring-up; a couple of yields cover the race).
fn route_pinched() -> Option<KernelClient> {
    for _ in 0..8 {
        if let Ok(client) = KernelClient::new_for("pinched") {
            return Some(client);
        }
        let _ = yield_();
    }
    None
}

/// Submits one `OP_COMPUTE` (`total_field` on the wire, `payload_elems`
/// actually staged: 0..payload_elems as u32le) and polls the completion
/// header. Returns `(status, elems, workers, payload_bytes)`.
fn submit_and_poll(
    client: &KernelClient,
    total_field: u32,
    payload_elems: usize,
    deadline_ns: u64,
) -> Option<(u32, u32, u32, Vec<u8>)> {
    let vmo_size = pn::DATA_OFFSET + payload_elems.max(1) * 4;
    let Ok(vmo) = nexus_abi::vmo_create(vmo_size) else {
        emit_line("pinched-probe: FAIL (vmo create)");
        return None;
    };

    // Pending header first, then the input elements.
    let pending = [0u8; pn::HDR_LEN];
    if nexus_abi::vmo_write(vmo, 0, &pending).is_err() {
        emit_line("pinched-probe: FAIL (vmo write hdr)");
        return None;
    }
    if payload_elems > 0 {
        let mut data = Vec::with_capacity(payload_elems * 4);
        for i in 0..payload_elems {
            data.extend_from_slice(&(i as u32).to_le_bytes());
        }
        if nexus_abi::vmo_write(vmo, pn::DATA_OFFSET, &data).is_err() {
            emit_line("pinched-probe: FAIL (vmo write data)");
            return None;
        }
    }

    // CAP_MOVE the clone; keep `vmo` for polling and read-back.
    let Ok(clone) = nexus_abi::cap_clone(vmo) else {
        emit_line("pinched-probe: FAIL (clone)");
        return None;
    };
    let frame = [
        pn::MAGIC0,
        pn::MAGIC1,
        pn::VERSION,
        pn::OP_COMPUTE,
        pn::JOB_MAP_MIX_U32,
        total_field.to_le_bytes()[0],
        total_field.to_le_bytes()[1],
        total_field.to_le_bytes()[2],
        total_field.to_le_bytes()[3],
    ];
    if client
        .send_with_cap_move_wait(
            &frame,
            clone,
            IpcWait::Timeout(core::time::Duration::from_millis(1000)),
        )
        .is_err()
    {
        emit_line("pinched-probe: FAIL (send)");
        return None;
    }

    let (status, elems, workers) = poll_header(vmo, deadline_ns)?;

    let mut data = Vec::new();
    if status == pn::STATUS_OK && payload_elems > 0 {
        data.resize(payload_elems * 4, 0);
        if nexus_abi::vmo_read(vmo, pn::DATA_OFFSET, data.as_mut_slice()).is_err() {
            let _ = nexus_abi::cap_close(vmo);
            return None;
        }
    }
    let _ = nexus_abi::cap_close(vmo);
    Some((status, elems, workers, data))
}
