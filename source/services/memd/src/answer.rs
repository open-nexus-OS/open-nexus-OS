// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: one request → one reply. [`Memd::answer`] decodes the frame, gates it on the
//! kernel-attributed sender (a band only from the lifecycle authority, a drill only from the
//! proof harness), applies it to the [`Authority`] and encodes the reply; the [`Outcome`]
//! names what happened so the loop writes a marker without re-deciding anything. A band push
//! answers nothing (fire-and-forget).
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/contract.rs

use nexus_wire::memd as wire;

use crate::authority::{Authority, BandEvent, Decision, Source};
use crate::gpu::GpuCharges;
use crate::order::Orders;

/// What one request did — the marker's input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A band was applied.
    Band(BandEvent),
    /// A band from anyone but the lifecycle authority was dropped.
    BandDenied,
    /// The state was answered.
    Status,
    /// A drill ran at `level` and came to `decision`.
    Drill { level: u8, decision: Decision },
    /// A drill from anyone but the proof harness was refused.
    DrillDenied,
    /// gpud charged `client` with `bytes` of GPU memory (6e-4); `total` is every client's
    /// together, `None` when the table had no row for a ninth client.
    GpuCharge { client: u64, bytes: u64, total: Option<u64> },
    /// A GPU charge from anyone but the GPU driver was dropped.
    GpuChargeDenied,
    /// The frame did not decode, or named a level this build does not know.
    Malformed,
    /// An op this version does not serve.
    Unsupported,
}

/// The identities the gates compare against (kernel service ids).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peers {
    /// The lifecycle authority — the one source of bands.
    pub abilitymgr: u64,
    /// The proof harness — the one source of drills.
    pub harness: u64,
    /// The GPU driver — the one source of GPU charges (6e-4).
    pub gpud: u64,
}

/// The service's whole state: the authority, the GPU charges and the peers it trusts.
#[derive(Debug)]
pub struct Memd {
    authority: Authority,
    gpu: GpuCharges,
    peers: Peers,
}

impl Memd {
    /// An authority that trusts `peers`; `sink` says whether the kernel's endpoint was bound.
    #[must_use]
    pub const fn new(peers: Peers, sink: bool) -> Self {
        Self { authority: Authority::new(sink), gpu: GpuCharges::new(), peers }
    }

    /// The authority (the loop feeds it the kernel's frames; the tests look at it).
    pub fn authority(&mut self) -> &mut Authority {
        &mut self.authority
    }

    /// What the GPU holds for each client, as gpud charged it.
    #[must_use]
    pub fn gpu(&self) -> &GpuCharges {
        &self.gpu
    }

    /// Answers `frame` from `sender` into `out`; returns the reply's length (0: no reply) and
    /// what happened. `orders` carries a drill's squeeze or kill order.
    pub fn answer(
        &mut self,
        frame: &[u8],
        sender: u64,
        out: &mut [u8],
        orders: &mut dyn Orders,
    ) -> (usize, Outcome) {
        let Some(&op) = frame.get(3) else {
            return (0, Outcome::Malformed);
        };
        if frame.len() < 4 || frame[0] != wire::MAGIC0 || frame[1] != wire::MAGIC1 {
            return (0, Outcome::Malformed);
        }
        match op {
            wire::OP_BAND => {
                if sender == 0 || sender != self.peers.abilitymgr {
                    return (0, Outcome::BandDenied);
                }
                let Some((pid, band)) = wire::decode_band(frame) else {
                    return (0, Outcome::Malformed);
                };
                (0, Outcome::Band(self.authority.on_band(pid, band)))
            }
            wire::OP_GPU_CHARGE => {
                if sender == 0 || sender != self.peers.gpud {
                    return (0, Outcome::GpuChargeDenied);
                }
                let Some((client, bytes)) = wire::decode_gpu_charge(frame) else {
                    return (0, Outcome::Malformed);
                };
                let total = self.gpu.set(client, bytes).ok();
                (0, Outcome::GpuCharge { client, bytes, total })
            }
            wire::OP_STATUS => {
                if wire::decode_status(frame).is_none() {
                    return (0, Outcome::Malformed);
                }
                let a = &self.authority;
                let (level, free, total) = a.level();
                let t = a.table();
                let reply = wire::encode_status_reply(
                    wire::OP_STATUS,
                    wire::STATUS_OK,
                    level,
                    u8::from(a.sink()),
                    t.len().min(u8::MAX as usize) as u8,
                    t.suspended().min(u8::MAX as usize) as u8,
                    a.resumed(),
                    a.kills(),
                    free,
                    total,
                );
                (write(out, &reply), Outcome::Status)
            }
            wire::OP_DRILL => {
                let Some(level) = wire::decode_drill(frame) else {
                    return (0, Outcome::Malformed);
                };
                if sender == 0 || sender != self.peers.harness {
                    let reply = wire::encode_drill_reply(wire::OP_DRILL, wire::STATUS_DENIED, 0, 0);
                    return (write(out, &reply), Outcome::DrillDenied);
                }
                if level > nexus_abi::syscall::memory::MEM_PRESSURE_EMERGENCY {
                    return (0, Outcome::Malformed);
                }
                let decision = self.authority.judge(level, Source::Drill, orders);
                let (status, pid, band) = match decision {
                    Decision::Kill { pid, band, ordered: true } => (wire::STATUS_OK, pid, band),
                    Decision::Kill { pid, band, ordered: false } => {
                        (wire::STATUS_FAILED, pid, band)
                    }
                    Decision::Squeeze { pid, band, squeezed: Some(_) } => {
                        (wire::STATUS_SQUEEZED, pid, band)
                    }
                    Decision::Squeeze { pid, band, squeezed: None } => {
                        (wire::STATUS_FAILED, pid, band)
                    }
                    Decision::SwapOut { pid, band, out: Some(_) } => {
                        (wire::STATUS_SWAPPED, pid, band)
                    }
                    Decision::SwapOut { pid, band, out: None } => (wire::STATUS_FAILED, pid, band),
                    Decision::NoVictim | Decision::None => (wire::STATUS_NO_VICTIM, 0, 0),
                };
                let reply = wire::encode_drill_reply(wire::OP_DRILL, status, pid, band);
                (write(out, &reply), Outcome::Drill { level, decision })
            }
            _ => (0, Outcome::Unsupported),
        }
    }
}

fn write(out: &mut [u8], reply: &[u8]) -> usize {
    let n = reply.len().min(out.len());
    out[..n].copy_from_slice(&reply[..n]);
    n
}
