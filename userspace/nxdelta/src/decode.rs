// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the bounded streaming `.nxdelta` decoder (RFC-0090) — the
//! device-side half. State machine over pushed byte slices: a header-sized
//! carry buffer is the ONLY internal storage; ADD payload streams through
//! as borrowed slices (no per-record allocation — os-service bump-heap
//! discipline). Every malformed shape (unknown tag, zero/oversized length,
//! out-of-base COPY, output overrun, bytes after END, truncation) rejects
//! as `Format`; the running output total must land EXACTLY on the header's
//! `target_size` at END or `finish()` refuses.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable (RFC-0090)
//! TEST_COVERAGE: unit tests below + tests/nxdelta_host bounds matrix
//! ADR: docs/rfcs/RFC-0090-nxdelta-boot-image-delta-stream-format.md

use crate::{DeltaError, Header, HEADER_LEN, MAX_RECORD_LEN, TAG_ADD, TAG_COPY, TAG_END};

/// One decoded stream element, borrowed from the pushed input.
#[derive(Debug, PartialEq, Eq)]
pub enum Event<'a> {
    /// The validated header — the caller performs base/target binding here.
    Header(Header),
    /// Copy `len` bytes from `base[base_off..]` to the output cursor.
    Copy { base_off: u64, len: u32 },
    /// Literal output bytes (one ADD record may arrive as several slices).
    Add(&'a [u8]),
}

/// Sink failure or malformed stream.
#[derive(Debug, PartialEq, Eq)]
pub enum PushError<E> {
    Format,
    Sink(E),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Header,
    Tag,
    CopyBody,
    AddLen,
    AddData,
    EndBody,
    Ended,
}

/// The streaming decoder.
pub struct Decoder {
    state: State,
    carry: [u8; HEADER_LEN],
    have: usize,
    header: Option<Header>,
    add_left: u32,
    out_total: u64,
}

impl Decoder {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: State::Header,
            carry: [0u8; HEADER_LEN],
            have: 0,
            header: None,
            add_left: 0,
            out_total: 0,
        }
    }

    fn need(state: State) -> usize {
        match state {
            State::Header => HEADER_LEN,
            State::Tag => 1,
            State::CopyBody => 12,
            State::AddLen => 4,
            State::EndBody => 8,
            State::AddData | State::Ended => 0,
        }
    }

    /// Feeds one input slice, emitting events into `sink`. A sink error
    /// aborts immediately (the caller owns cleanup semantics).
    pub fn push<E>(
        &mut self,
        mut bytes: &[u8],
        sink: &mut impl FnMut(Event<'_>) -> Result<(), E>,
    ) -> Result<(), PushError<E>> {
        while !bytes.is_empty() {
            match self.state {
                State::Ended => return Err(PushError::Format),
                State::AddData => {
                    let take = (self.add_left as usize).min(bytes.len());
                    sink(Event::Add(&bytes[..take])).map_err(PushError::Sink)?;
                    self.add_left -= take as u32;
                    bytes = &bytes[take..];
                    if self.add_left == 0 {
                        self.state = State::Tag;
                        self.have = 0;
                    }
                }
                state => {
                    let need = Self::need(state);
                    let take = (need - self.have).min(bytes.len());
                    self.carry[self.have..self.have + take].copy_from_slice(&bytes[..take]);
                    self.have += take;
                    bytes = &bytes[take..];
                    if self.have == need {
                        self.step(sink)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Consumes one completed carry window for the current state.
    fn step<E>(
        &mut self,
        sink: &mut impl FnMut(Event<'_>) -> Result<(), E>,
    ) -> Result<(), PushError<E>> {
        let header_target = self.header.map(|h| h.target_size);
        match self.state {
            State::Header => {
                let header =
                    Header::decode(&self.carry[..HEADER_LEN]).map_err(|_| PushError::Format)?;
                if header.base_size == 0 || header.target_size == 0 {
                    return Err(PushError::Format);
                }
                self.header = Some(header);
                sink(Event::Header(header)).map_err(PushError::Sink)?;
                self.state = State::Tag;
            }
            State::Tag => {
                self.state = match self.carry[0] {
                    TAG_COPY => State::CopyBody,
                    TAG_ADD => State::AddLen,
                    TAG_END => State::EndBody,
                    _ => return Err(PushError::Format),
                };
            }
            State::CopyBody => {
                let base_off = u64::from_le_bytes(self.carry[0..8].try_into().unwrap_or([0; 8]));
                let len = u32::from_le_bytes(self.carry[8..12].try_into().unwrap_or([0; 4]));
                let header = self.header.ok_or(PushError::Format)?;
                if len == 0
                    || len > MAX_RECORD_LEN
                    || base_off.checked_add(u64::from(len)).is_none_or(|e| e > header.base_size)
                    || self.out_total + u64::from(len) > header.target_size
                {
                    return Err(PushError::Format);
                }
                self.out_total += u64::from(len);
                sink(Event::Copy { base_off, len }).map_err(PushError::Sink)?;
                self.state = State::Tag;
            }
            State::AddLen => {
                let len = u32::from_le_bytes(self.carry[0..4].try_into().unwrap_or([0; 4]));
                let target = header_target.ok_or(PushError::Format)?;
                if len == 0 || len > MAX_RECORD_LEN || self.out_total + u64::from(len) > target {
                    return Err(PushError::Format);
                }
                self.out_total += u64::from(len);
                self.add_left = len;
                self.state = State::AddData;
            }
            State::EndBody => {
                let out = u64::from_le_bytes(self.carry[0..8].try_into().unwrap_or([0; 8]));
                let target = header_target.ok_or(PushError::Format)?;
                if out != self.out_total || out != target {
                    return Err(PushError::Format);
                }
                self.state = State::Ended;
            }
            State::AddData | State::Ended => return Err(PushError::Format),
        }
        self.have = 0;
        Ok(())
    }

    /// `Ok` only when the stream closed cleanly (END seen, totals exact).
    pub fn finish(&self) -> Result<(), DeltaError> {
        if self.state == State::Ended {
            Ok(())
        } else {
            Err(DeltaError::Format)
        }
    }
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ALGO_STORED;

    fn header(base: u64, target: u64) -> Header {
        Header {
            algo: ALGO_STORED,
            base_size: base,
            target_size: target,
            base_sha256: [1u8; 32],
            target_sha256: [2u8; 32],
        }
    }

    fn stream(records: &[u8], base: u64, target: u64) -> Vec<u8> {
        let mut out = header(base, target).encode().to_vec();
        out.extend_from_slice(records);
        out
    }

    fn drive(bytes: &[u8], piece: usize) -> Result<(Vec<String>, Decoder), ()> {
        let mut dec = Decoder::new();
        let mut log = Vec::new();
        for chunk in bytes.chunks(piece.max(1)) {
            dec.push(chunk, &mut |ev: Event<'_>| {
                log.push(match ev {
                    Event::Header(_) => "hdr".to_string(),
                    Event::Copy { base_off, len } => format!("copy {base_off}+{len}"),
                    Event::Add(b) => format!("add {}", b.len()),
                });
                Ok::<(), ()>(())
            })
            .map_err(|_| ())?;
        }
        Ok((log, dec))
    }

    fn records(target_adds: &[&[u8]], copies: &[(u64, u32)], out_total: u64) -> Vec<u8> {
        let mut r = Vec::new();
        for &(off, len) in copies {
            r.push(TAG_COPY);
            r.extend_from_slice(&off.to_le_bytes());
            r.extend_from_slice(&len.to_le_bytes());
        }
        for add in target_adds {
            r.push(TAG_ADD);
            r.extend_from_slice(&(add.len() as u32).to_le_bytes());
            r.extend_from_slice(add);
        }
        r.push(TAG_END);
        r.extend_from_slice(&out_total.to_le_bytes());
        r
    }

    #[test]
    fn streams_across_arbitrary_chunk_boundaries() {
        let recs = records(&[b"xyz"], &[(4, 7)], 10);
        let bytes = stream(&recs, 100, 10);
        // Every split width must decode to the same event log.
        let (whole, dec) = drive(&bytes, bytes.len()).expect("whole");
        assert!(dec.finish().is_ok());
        for piece in 1..=13 {
            let (log, dec) = drive(&bytes, piece).expect("pieced");
            assert!(dec.finish().is_ok(), "piece={piece}");
            // ADD slices may split differently; compare copies+total adds.
            let adds: usize = log
                .iter()
                .filter_map(|l| l.strip_prefix("add ").and_then(|n| n.parse::<usize>().ok()))
                .sum();
            assert_eq!(adds, 3, "piece={piece}");
            assert!(log.contains(&"copy 4+7".to_string()), "piece={piece}");
            let _ = &whole;
        }
    }

    #[test]
    fn test_reject_bounds_matrix() {
        // (records, base, target) shapes that must all reject as Format.
        let cases: Vec<(Vec<u8>, u64, u64)> = vec![
            // COPY beyond base
            (records(&[], &[(95, 10)], 10), 100, 10),
            // zero-length COPY
            (records(&[], &[(0, 0)], 0), 100, 10),
            // output overrun
            (records(&[b"abcdef"], &[(0, 7)], 13), 100, 10),
            // END total mismatch
            (records(&[b"ab"], &[], 3), 100, 2),
            // unknown tag
            (
                {
                    let mut r = records(&[b"ab"], &[], 2);
                    r.insert(0, 0x77);
                    r
                },
                100,
                2,
            ),
            // bytes after END
            (
                {
                    let mut r = records(&[b"ab"], &[], 2);
                    r.push(0);
                    r
                },
                100,
                2,
            ),
        ];
        for (i, (recs, base, target)) in cases.into_iter().enumerate() {
            let bytes = stream(&recs, base, target);
            assert!(drive(&bytes, 5).is_err(), "case {i} must reject");
        }
        // Truncation: END never arrives ⇒ finish refuses.
        let recs = records(&[b"ab"], &[], 2);
        let bytes = stream(&recs[..recs.len() - 4], 100, 2);
        let (_, dec) = drive(&bytes, 3).expect("push side is fine");
        assert!(dec.finish().is_err(), "truncated stream must not finish");
    }
}
