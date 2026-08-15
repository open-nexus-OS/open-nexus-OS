// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Binary `.nxcd` container: fixed header, kind-sorted section table,
//! canonically packed payloads. Encode and decode are deterministic and every
//! bound is checked before any payload is copied.
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Roundtrip + reject unit tests below; integration in `tests/crashdump_v2_host`
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

use crate::NxcdError;
use std::collections::BTreeMap;

const MAGIC: [u8; 4] = *b"NXCD";
const VERSION: u16 = 1;
const HEADER_LEN: usize = 16;
const ENTRY_LEN: usize = 12;
const SECTION_KIND_COUNT: usize = 6;

/// Hard cap for a whole `.nxcd` container (bounds untrusted parsing).
pub const MAX_TOTAL_NXCD: usize = 1024 * 1024;

/// Named sections of a `.nxcd` container, in canonical (encoded) order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SectionKind {
    /// `header.json` — stable-key crash summary (required).
    Header,
    /// `frames.json` — program counters, symbolized if available (required).
    Frames,
    /// `maps.json` — module list with build-ids (required).
    Maps,
    /// `logs.jsonl` — bounded log excerpt (optional).
    Logs,
    /// `spans.jsonl` — bounded trace-span excerpt (optional).
    Spans,
    /// `regs.bin` — bounded raw register snapshot (optional).
    Regs,
}

impl SectionKind {
    /// All kinds in canonical order.
    pub const ALL: [SectionKind; SECTION_KIND_COUNT] = [
        SectionKind::Header,
        SectionKind::Frames,
        SectionKind::Maps,
        SectionKind::Logs,
        SectionKind::Spans,
        SectionKind::Regs,
    ];

    fn from_u8(raw: u8) -> Result<Self, NxcdError> {
        match raw {
            0 => Ok(Self::Header),
            1 => Ok(Self::Frames),
            2 => Ok(Self::Maps),
            3 => Ok(Self::Logs),
            4 => Ok(Self::Spans),
            5 => Ok(Self::Regs),
            _ => Err(NxcdError::UnknownSection),
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Self::Header => 0,
            Self::Frames => 1,
            Self::Maps => 2,
            Self::Logs => 3,
            Self::Spans => 4,
            Self::Regs => 5,
        }
    }

    /// Stable section file name (documentation + `nx crash show` labels).
    pub fn name(self) -> &'static str {
        match self {
            Self::Header => "header.json",
            Self::Frames => "frames.json",
            Self::Maps => "maps.json",
            Self::Logs => "logs.jsonl",
            Self::Spans => "spans.jsonl",
            Self::Regs => "regs.bin",
        }
    }

    /// Per-kind payload bound in bytes.
    pub fn max_len(self) -> usize {
        match self {
            Self::Header => 16 * 1024,
            Self::Frames => 64 * 1024,
            Self::Maps => 64 * 1024,
            Self::Logs => 256 * 1024,
            Self::Spans => 256 * 1024,
            Self::Regs => 4 * 1024,
        }
    }

    /// Required sections must be present in every valid container.
    pub fn required(self) -> bool {
        matches!(self, Self::Header | Self::Frames | Self::Maps)
    }
}

/// In-memory `.nxcd` container: canonical map of section payloads.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NxcdContainer {
    sections: BTreeMap<SectionKind, Vec<u8>>,
}

impl NxcdContainer {
    /// Empty container (invalid to encode until required sections are set).
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a section payload; rejects oversize payloads eagerly.
    pub fn insert(&mut self, kind: SectionKind, payload: Vec<u8>) -> Result<(), NxcdError> {
        if payload.len() > kind.max_len() {
            return Err(NxcdError::OversizeSection);
        }
        self.sections.insert(kind, payload);
        Ok(())
    }

    /// Section payload, if present.
    pub fn get(&self, kind: SectionKind) -> Option<&[u8]> {
        self.sections.get(&kind).map(Vec::as_slice)
    }

    /// Present section kinds in canonical order.
    pub fn kinds(&self) -> Vec<SectionKind> {
        self.sections.keys().copied().collect()
    }

    /// Deterministic encode: fixed header, kind-sorted table, packed payloads.
    pub fn encode(&self) -> Result<Vec<u8>, NxcdError> {
        for kind in SectionKind::ALL {
            if kind.required() && !self.sections.contains_key(&kind) {
                return Err(NxcdError::MissingRequired);
            }
        }
        let count = self.sections.len();
        let mut total = HEADER_LEN + count * ENTRY_LEN;
        for (kind, payload) in &self.sections {
            if payload.len() > kind.max_len() {
                return Err(NxcdError::OversizeSection);
            }
            total += payload.len();
        }
        if total > MAX_TOTAL_NXCD {
            return Err(NxcdError::OversizeSection);
        }

        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&(count as u16).to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());

        let mut offset = HEADER_LEN + count * ENTRY_LEN;
        for (kind, payload) in &self.sections {
            out.push(kind.as_u8());
            out.extend_from_slice(&[0u8; 3]);
            out.extend_from_slice(&(offset as u32).to_le_bytes());
            out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            offset += payload.len();
        }
        for payload in self.sections.values() {
            out.extend_from_slice(payload);
        }
        debug_assert_eq!(out.len(), total);
        Ok(out)
    }

    /// Bounded decode of untrusted bytes; enforces the canonical layout so
    /// `decode(encode(c)) == c` and no overlap/gap games are representable.
    pub fn decode(bytes: &[u8]) -> Result<Self, NxcdError> {
        if bytes.len() > MAX_TOTAL_NXCD {
            return Err(NxcdError::OversizeSection);
        }
        if bytes.len() < HEADER_LEN {
            return Err(NxcdError::Truncated);
        }
        if bytes[0..4] != MAGIC {
            return Err(NxcdError::BadMagic);
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != VERSION {
            return Err(NxcdError::UnsupportedVersion);
        }
        let count = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        let total = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        if count > SECTION_KIND_COUNT {
            return Err(NxcdError::UnknownSection);
        }
        if total != bytes.len() {
            return Err(NxcdError::LengthMismatch);
        }
        let table_end = HEADER_LEN + count * ENTRY_LEN;
        if bytes.len() < table_end {
            return Err(NxcdError::Truncated);
        }

        let mut sections = BTreeMap::new();
        let mut expected_offset = table_end;
        let mut last_kind: Option<SectionKind> = None;
        for i in 0..count {
            let entry = &bytes[HEADER_LEN + i * ENTRY_LEN..HEADER_LEN + (i + 1) * ENTRY_LEN];
            let kind = SectionKind::from_u8(entry[0])?;
            if let Some(prev) = last_kind {
                if kind <= prev {
                    return Err(NxcdError::SectionOrder);
                }
            }
            last_kind = Some(kind);
            let offset = u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]) as usize;
            let len = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
            if len > kind.max_len() {
                return Err(NxcdError::OversizeSection);
            }
            if offset != expected_offset {
                return Err(NxcdError::NonCanonicalLayout);
            }
            let end = offset.checked_add(len).ok_or(NxcdError::OversizeSection)?;
            if end > bytes.len() {
                return Err(NxcdError::Truncated);
            }
            sections.insert(kind, bytes[offset..end].to_vec());
            expected_offset = end;
        }
        if expected_offset != bytes.len() {
            return Err(NxcdError::NonCanonicalLayout);
        }
        for kind in SectionKind::ALL {
            if kind.required() && !sections.contains_key(&kind) {
                return Err(NxcdError::MissingRequired);
            }
        }
        Ok(Self { sections })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> NxcdContainer {
        let mut c = NxcdContainer::new();
        c.insert(SectionKind::Header, b"{\"pid\":7}".to_vec()).expect("header");
        c.insert(SectionKind::Frames, b"{\"frames\":[]}".to_vec()).expect("frames");
        c.insert(SectionKind::Maps, b"{\"modules\":[]}".to_vec()).expect("maps");
        c.insert(SectionKind::Regs, vec![0xAB; 32]).expect("regs");
        c
    }

    #[test]
    fn test_nxcd_roundtrip_identical() {
        let c = sample();
        let bytes = c.encode().expect("encode");
        let got = NxcdContainer::decode(&bytes).expect("decode");
        assert_eq!(got, c);
        // Deterministic: encoding twice yields identical bytes.
        assert_eq!(bytes, got.encode().expect("re-encode"));
    }

    #[test]
    fn test_reject_nxcd_missing_required_section() {
        let mut c = NxcdContainer::new();
        c.insert(SectionKind::Header, b"{}".to_vec()).expect("header");
        assert_eq!(c.encode(), Err(NxcdError::MissingRequired));
    }

    #[test]
    fn test_reject_nxcd_oversize_section() {
        let mut c = sample();
        assert_eq!(
            c.insert(SectionKind::Regs, vec![0u8; SectionKind::Regs.max_len() + 1]),
            Err(NxcdError::OversizeSection)
        );
    }

    #[test]
    fn test_reject_nxcd_bad_magic_and_version() {
        let mut bytes = sample().encode().expect("encode");
        bytes[0] = b'X';
        assert_eq!(NxcdContainer::decode(&bytes), Err(NxcdError::BadMagic));
        let mut bytes = sample().encode().expect("encode");
        bytes[4] = 0xFF;
        assert_eq!(NxcdContainer::decode(&bytes), Err(NxcdError::UnsupportedVersion));
    }

    #[test]
    fn test_reject_nxcd_truncated_and_length_mismatch() {
        let bytes = sample().encode().expect("encode");
        assert_eq!(NxcdContainer::decode(&bytes[..8]), Err(NxcdError::Truncated));
        assert_eq!(
            NxcdContainer::decode(&bytes[..bytes.len() - 1]),
            Err(NxcdError::LengthMismatch)
        );
    }

    #[test]
    fn test_reject_nxcd_unknown_section_kind() {
        let mut bytes = sample().encode().expect("encode");
        // First table entry kind byte -> invalid.
        bytes[HEADER_LEN] = 9;
        assert_eq!(NxcdContainer::decode(&bytes), Err(NxcdError::UnknownSection));
    }

    #[test]
    fn test_reject_nxcd_duplicate_or_unsorted_sections() {
        let mut bytes = sample().encode().expect("encode");
        // Make the second entry repeat the first kind (duplicate == not ascending).
        bytes[HEADER_LEN + ENTRY_LEN] = bytes[HEADER_LEN];
        assert_eq!(NxcdContainer::decode(&bytes), Err(NxcdError::SectionOrder));
    }

    #[test]
    fn test_reject_nxcd_non_canonical_offset() {
        let mut bytes = sample().encode().expect("encode");
        // Shift the first payload offset by one byte.
        let off = u32::from_le_bytes([
            bytes[HEADER_LEN + 4],
            bytes[HEADER_LEN + 5],
            bytes[HEADER_LEN + 6],
            bytes[HEADER_LEN + 7],
        ]);
        bytes[HEADER_LEN + 4..HEADER_LEN + 8].copy_from_slice(&(off + 1).to_le_bytes());
        assert_eq!(NxcdContainer::decode(&bytes), Err(NxcdError::NonCanonicalLayout));
    }

    #[test]
    fn test_reject_nxcd_oversize_input() {
        let bytes = vec![0u8; MAX_TOTAL_NXCD + 1];
        assert_eq!(NxcdContainer::decode(&bytes), Err(NxcdError::OversizeSection));
    }
}
