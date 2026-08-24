// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Typed JSON section payloads for `.nxcd` (stable keys, bounded)
//! plus the minidump-v1 → `.nxcd` conversion that carries `build_id` through
//! verbatim (the symbolization key; never re-derived here).
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests below; integration in `tests/crashdump_v2_host`
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};

use crate::container::{NxcdContainer, SectionKind};
use crate::NxcdError;

/// `header.json` — stable key order is the struct field order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashHeader {
    pub format: String,
    pub format_version: u32,
    pub timestamp_nsec: u64,
    pub pid: u32,
    pub code: i32,
    pub name: String,
    pub build_id: String,
    /// Kernel-attributed exit reason (ADR-0056; TASK-0051B). Optional so
    /// pre-0051B headers keep decoding and their JSON stays byte-identical.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// One entry of `frames.json`; `function`/`file`/`line` stay `null` until a
/// symbolizer (nxsym) fills them in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameRecord {
    pub pc: u64,
    pub build_id: String,
    pub function: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// `frames.json` root object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FramesSection {
    pub frames: Vec<FrameRecord>,
}

/// One module of `maps.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleRecord {
    pub name: String,
    pub build_id: String,
}

/// `maps.json` root object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapsSection {
    pub modules: Vec<ModuleRecord>,
}

impl CrashHeader {
    /// Bounded parse of an untrusted `header.json` payload.
    pub fn from_section(container: &NxcdContainer) -> Result<Self, NxcdError> {
        let bytes = container.get(SectionKind::Header).ok_or(NxcdError::MissingRequired)?;
        serde_json::from_slice(bytes).map_err(|_| NxcdError::JsonDecode)
    }
}

impl FramesSection {
    /// Bounded parse of an untrusted `frames.json` payload.
    pub fn from_section(container: &NxcdContainer) -> Result<Self, NxcdError> {
        let bytes = container.get(SectionKind::Frames).ok_or(NxcdError::MissingRequired)?;
        serde_json::from_slice(bytes).map_err(|_| NxcdError::JsonDecode)
    }
}

impl MapsSection {
    /// Bounded parse of an untrusted `maps.json` payload.
    pub fn from_section(container: &NxcdContainer) -> Result<Self, NxcdError> {
        let bytes = container.get(SectionKind::Maps).ok_or(NxcdError::MissingRequired)?;
        serde_json::from_slice(bytes).map_err(|_| NxcdError::JsonDecode)
    }
}

/// Convert a validated minidump-v1 frame into a `.nxcd` container.
///
/// The `build_id` recorded by the producer (execd stamps
/// `crash::deterministic_build_id(name)` for ELFs without an embedded id) is
/// carried through verbatim into `header.json`, every `frames.json` record,
/// and the `maps.json` module list — symbolizers key on it, so it must never
/// be re-derived on the host.
pub fn from_minidump(frame: &crash::MinidumpFrame) -> Result<NxcdContainer, NxcdError> {
    from_minidump_with_reason(frame, None)
}

/// `from_minidump` with the kernel-attributed exit reason attached to
/// `header.json` (the on-device writer's entry point, TASK-0051B).
pub fn from_minidump_with_reason(
    frame: &crash::MinidumpFrame,
    reason: Option<&str>,
) -> Result<NxcdContainer, NxcdError> {
    frame.validate().map_err(|_| NxcdError::JsonEncode)?;
    let header = CrashHeader {
        format: String::from("nxcd"),
        format_version: 1,
        timestamp_nsec: frame.timestamp_nsec,
        pid: frame.pid,
        code: frame.code,
        name: frame.name.clone(),
        build_id: frame.build_id.clone(),
        reason: reason.map(String::from),
    };
    let frames = FramesSection {
        frames: frame
            .pcs
            .iter()
            .map(|pc| FrameRecord {
                pc: *pc,
                build_id: frame.build_id.clone(),
                function: None,
                file: None,
                line: None,
            })
            .collect(),
    };
    let maps = MapsSection {
        modules: vec![ModuleRecord { name: frame.name.clone(), build_id: frame.build_id.clone() }],
    };

    let mut container = NxcdContainer::new();
    container.insert(
        SectionKind::Header,
        serde_json::to_vec(&header).map_err(|_| NxcdError::JsonEncode)?,
    )?;
    container.insert(
        SectionKind::Frames,
        serde_json::to_vec(&frames).map_err(|_| NxcdError::JsonEncode)?,
    )?;
    container
        .insert(SectionKind::Maps, serde_json::to_vec(&maps).map_err(|_| NxcdError::JsonEncode)?)?;
    Ok(container)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_minidump() -> crash::MinidumpFrame {
        crash::MinidumpFrame {
            timestamp_nsec: 1234,
            pid: 7,
            code: 42,
            name: String::from("demo.exit42"),
            build_id: crash::deterministic_build_id("demo.exit42"),
            pcs: vec![0x10, 0x20],
            stack_preview: vec![0xAA; 8],
            code_preview: vec![0xCC; 4],
        }
    }

    #[test]
    fn test_from_minidump_carries_build_id_verbatim() {
        let dump = sample_minidump();
        let container = from_minidump(&dump).expect("convert");
        let header = CrashHeader::from_section(&container).expect("header");
        assert_eq!(header.build_id, dump.build_id);
        assert_eq!(header.pid, 7);
        assert_eq!(header.code, 42);
        let frames = FramesSection::from_section(&container).expect("frames");
        assert_eq!(frames.frames.len(), 2);
        assert!(frames.frames.iter().all(|f| f.build_id == dump.build_id));
        let maps = MapsSection::from_section(&container).expect("maps");
        assert_eq!(maps.modules.len(), 1);
        assert_eq!(maps.modules[0].build_id, dump.build_id);
    }

    #[test]
    fn test_from_minidump_is_deterministic() {
        let dump = sample_minidump();
        let a = from_minidump(&dump).expect("a").encode().expect("encode a");
        let b = from_minidump(&dump).expect("b").encode().expect("encode b");
        assert_eq!(a, b);
    }

    #[test]
    fn test_reject_malformed_header_json() {
        let mut container = from_minidump(&sample_minidump()).expect("convert");
        container.insert(SectionKind::Header, b"not json".to_vec()).expect("insert");
        assert_eq!(CrashHeader::from_section(&container), Err(NxcdError::JsonDecode));
    }

    /// TASK-0051B: the reason rides header.json; without one the JSON stays
    /// byte-identical to pre-0051B output (skip_serializing_if) and old
    /// headers keep decoding (serde default).
    #[test]
    fn test_reason_roundtrip_and_absent_compatibility() {
        let dump = sample_minidump();
        let with = from_minidump_with_reason(&dump, Some("fault(load-page)")).expect("convert");
        let header = CrashHeader::from_section(&with).expect("header");
        assert_eq!(header.reason.as_deref(), Some("fault(load-page)"));

        let without = from_minidump(&dump).expect("convert");
        let plain = CrashHeader::from_section(&without).expect("header");
        assert_eq!(plain.reason, None);
        let json = without.get(SectionKind::Header).expect("section");
        assert!(!String::from_utf8_lossy(json).contains("reason"));
    }
}
