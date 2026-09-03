// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: deterministic `.nxdelta` emission (RFC-0090) — the host half
//! (`nx image ota --delta-from`). rsync-shape rolling checksum over fixed
//! 4096-byte base blocks + byte-exact confirmation, greedy left-to-right
//! target scan with forward match extension and adjacent-COPY coalescing,
//! literals flushed as bounded ADD records. No wall clock, no randomness,
//! candidate lists in ascending base order — emitting twice yields
//! identical bytes (the determinism DoD).
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Stable (RFC-0090)
//! TEST_COVERAGE: tests/nxdelta_host (roundtrip, determinism, tamper)
//! ADR: docs/rfcs/RFC-0090-nxdelta-boot-image-delta-stream-format.md

use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::decode::{Decoder, Event, PushError};
use crate::{DeltaError, Header, ALGO_STORED, MAX_RECORD_LEN, TAG_ADD, TAG_COPY, TAG_END};

/// The base is indexed in fixed blocks of this size (RFC-0090 normative
/// for deterministic emission).
pub const BLOCK: usize = 4096;

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

/// rsync-shape rolling checksum: `a` = byte sum, `b` = position-weighted
/// sum, both mod 2^16, packed `a | b << 16`.
fn rollsum(window: &[u8]) -> u32 {
    let mut a: u32 = 0;
    let mut b: u32 = 0;
    let len = window.len() as u32;
    for (i, &x) in window.iter().enumerate() {
        a = a.wrapping_add(u32::from(x));
        b = b.wrapping_add((len - i as u32).wrapping_mul(u32::from(x)));
    }
    (a & 0xffff) | ((b & 0xffff) << 16)
}

/// One rolling step: drop `out`, append `inb` (window length [`BLOCK`]).
fn roll(sum: u32, out: u8, inb: u8) -> u32 {
    let mut a = sum & 0xffff;
    let mut b = (sum >> 16) & 0xffff;
    a = a.wrapping_sub(u32::from(out)).wrapping_add(u32::from(inb)) & 0xffff;
    b = b.wrapping_sub((BLOCK as u32).wrapping_mul(u32::from(out))).wrapping_add(a) & 0xffff;
    a | (b << 16)
}

struct Emitter {
    out: Vec<u8>,
    /// Pending literal window into the target: `[lit_start, pos)`.
    lit: Vec<u8>,
    /// Last emitted COPY, held for coalescing: (base_off, len).
    copy: Option<(u64, u64)>,
}

impl Emitter {
    fn flush_copy(&mut self) {
        if let Some((off, len)) = self.copy.take() {
            let mut remaining = len;
            let mut at = off;
            while remaining > 0 {
                let take = remaining.min(u64::from(MAX_RECORD_LEN));
                self.out.push(TAG_COPY);
                self.out.extend_from_slice(&at.to_le_bytes());
                self.out.extend_from_slice(&(take as u32).to_le_bytes());
                at += take;
                remaining -= take;
            }
        }
    }

    fn flush_lit(&mut self) {
        for chunk in core::mem::take(&mut self.lit).chunks(MAX_RECORD_LEN as usize) {
            self.out.push(TAG_ADD);
            self.out.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
            self.out.extend_from_slice(chunk);
        }
    }

    fn add(&mut self, byte: u8) {
        self.flush_copy();
        self.lit.push(byte);
    }

    fn copy(&mut self, base_off: u64, len: u64) {
        if let Some((off, prev)) = self.copy {
            if off + prev == base_off {
                self.copy = Some((off, prev + len));
                return;
            }
        }
        self.flush_copy();
        self.flush_lit();
        self.copy = Some((base_off, len));
    }
}

/// Emits the deterministic `.nxdelta` stream for `base -> target`.
#[must_use]
pub fn make(base: &[u8], target: &[u8]) -> Vec<u8> {
    // Index: rollsum -> base block starts, ascending (deterministic pick).
    let mut index: HashMap<u32, Vec<usize>> = HashMap::new();
    for start in (0..base.len().saturating_sub(BLOCK - 1)).step_by(BLOCK) {
        index.entry(rollsum(&base[start..start + BLOCK])).or_default().push(start);
    }

    let header = Header {
        algo: ALGO_STORED,
        base_size: base.len() as u64,
        target_size: target.len() as u64,
        base_sha256: sha256(base),
        target_sha256: sha256(target),
    };
    let mut em = Emitter { out: header.encode().to_vec(), lit: Vec::new(), copy: None };

    let mut pos = 0usize;
    let mut sum: Option<u32> = None;
    while pos < target.len() {
        if pos + BLOCK <= target.len() {
            let s = match sum {
                Some(s) => s,
                None => rollsum(&target[pos..pos + BLOCK]),
            };
            let matched = index.get(&s).and_then(|starts| {
                starts.iter().copied().find(|&b| base[b..b + BLOCK] == target[pos..pos + BLOCK])
            });
            if let Some(base_at) = matched {
                // Greedy forward extension past the block.
                let mut len = BLOCK;
                while base_at + len < base.len()
                    && pos + len < target.len()
                    && base[base_at + len] == target[pos + len]
                {
                    len += 1;
                }
                em.copy(base_at as u64, len as u64);
                pos += len;
                sum = None;
                continue;
            }
            // No match: the leading byte becomes a literal, roll forward.
            em.add(target[pos]);
            if pos + BLOCK < target.len() {
                sum = Some(roll(s, target[pos], target[pos + BLOCK]));
            } else {
                sum = None;
            }
            pos += 1;
        } else {
            em.add(target[pos]);
            pos += 1;
        }
    }
    em.flush_copy();
    em.flush_lit();
    em.out.push(TAG_END);
    em.out.extend_from_slice(&(target.len() as u64).to_le_bytes());
    em.out
}

/// Whole-buffer apply (host verification + tests): reconstructs the target
/// and checks BOTH digests — the same guarantees the device path enforces
/// through its own base binding and readback gate.
pub fn apply(base: &[u8], delta: &[u8]) -> Result<Vec<u8>, DeltaError> {
    let mut dec = Decoder::new();
    let mut out: Vec<u8> = Vec::new();
    let mut header: Option<Header> = None;
    let result = dec.push(delta, &mut |ev: Event<'_>| -> Result<(), DeltaError> {
        match ev {
            Event::Header(h) => {
                if h.base_size != base.len() as u64 || h.base_sha256 != sha256(base) {
                    return Err(DeltaError::Format);
                }
                header = Some(h);
            }
            Event::Copy { base_off, len } => {
                let start = base_off as usize;
                out.extend_from_slice(&base[start..start + len as usize]);
            }
            Event::Add(bytes) => out.extend_from_slice(bytes),
        }
        Ok(())
    });
    match result {
        Ok(()) => {}
        Err(PushError::Format) | Err(PushError::Sink(_)) => return Err(DeltaError::Format),
    }
    dec.finish()?;
    let header = header.ok_or(DeltaError::Format)?;
    if out.len() as u64 != header.target_size || sha256(&out) != header.target_sha256 {
        return Err(DeltaError::Format);
    }
    Ok(out)
}
