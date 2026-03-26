// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Deterministic minidump v1 framing + path normalization helpers
//! OWNERS: @runtime
//! STATUS: Functional (host-first + os-lite writer seam)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Host unit tests including required reject-path cases
//! ADR: docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

const MAGIC: [u8; 4] = *b"NMD1";
const VERSION: u8 = 1;
const HEADER_LEN: usize = 32;

pub const MAX_BUILD_ID_LEN: usize = 64;
pub const MAX_NAME_LEN: usize = 48;
pub const MAX_PC_COUNT: usize = 32;
pub const MAX_STACK_PREVIEW: usize = 4096;
pub const MAX_CODE_PREVIEW: usize = 256;
pub const MAX_TOTAL_FRAME: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CrashError {
    MalformedHeader,
    UnsupportedVersion,
    TruncatedFrame,
    InvalidName,
    InvalidBuildId,
    OversizeStackPreview,
    OversizeCodePreview,
    OversizeTotalFrame,
    InvalidPathEscape,
    PathOutOfScope,
    #[cfg(all(feature = "os-lite", nexus_env = "os"))]
    StatefsWriteFailed,
    #[cfg(all(feature = "os-lite", nexus_env = "os"))]
    StatefsReadFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinidumpFrame {
    pub timestamp_nsec: u64,
    pub pid: u32,
    pub code: i32,
    pub name: String,
    pub build_id: String,
    pub pcs: Vec<u64>,
    pub stack_preview: Vec<u8>,
    pub code_preview: Vec<u8>,
}

impl MinidumpFrame {
    pub fn validate(&self) -> Result<(), CrashError> {
        if self.name.is_empty() || self.name.len() > MAX_NAME_LEN {
            return Err(CrashError::InvalidName);
        }
        if self
            .name
            .as_bytes()
            .iter()
            .any(|b| !matches!(*b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_'))
        {
            return Err(CrashError::InvalidName);
        }
        if self.build_id.is_empty() || self.build_id.len() > MAX_BUILD_ID_LEN {
            return Err(CrashError::InvalidBuildId);
        }
        if self
            .build_id
            .as_bytes()
            .iter()
            .any(|b| !matches!(*b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_'))
        {
            return Err(CrashError::InvalidBuildId);
        }
        if self.pcs.len() > MAX_PC_COUNT {
            return Err(CrashError::OversizeTotalFrame);
        }
        if self.stack_preview.len() > MAX_STACK_PREVIEW {
            return Err(CrashError::OversizeStackPreview);
        }
        if self.code_preview.len() > MAX_CODE_PREVIEW {
            return Err(CrashError::OversizeCodePreview);
        }
        let body_len = self.build_id.len()
            + self.name.len()
            + self.pcs.len() * 8
            + self.stack_preview.len()
            + self.code_preview.len();
        let total = HEADER_LEN + body_len;
        if total > MAX_TOTAL_FRAME || total > u16::MAX as usize {
            return Err(CrashError::OversizeTotalFrame);
        }
        Ok(())
    }

    pub fn encode(&self) -> Result<Vec<u8>, CrashError> {
        self.validate()?;

        let body_len = self.build_id.len()
            + self.name.len()
            + self.pcs.len() * 8
            + self.stack_preview.len()
            + self.code_preview.len();
        let total_len = HEADER_LEN + body_len;
        let mut out = Vec::with_capacity(total_len);

        out.extend_from_slice(&MAGIC);
        out.push(VERSION);
        out.push(self.build_id.len() as u8);
        out.push(self.name.len() as u8);
        out.push(self.pcs.len() as u8);
        out.extend_from_slice(&(self.stack_preview.len() as u16).to_le_bytes());
        out.extend_from_slice(&(self.code_preview.len() as u16).to_le_bytes());
        out.extend_from_slice(&self.pid.to_le_bytes());
        out.extend_from_slice(&self.code.to_le_bytes());
        out.extend_from_slice(&self.timestamp_nsec.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(total_len as u16).to_le_bytes());

        out.extend_from_slice(self.build_id.as_bytes());
        out.extend_from_slice(self.name.as_bytes());
        for pc in &self.pcs {
            out.extend_from_slice(&pc.to_le_bytes());
        }
        out.extend_from_slice(&self.stack_preview);
        out.extend_from_slice(&self.code_preview);
        Ok(out)
    }

    pub fn decode(frame: &[u8]) -> Result<Self, CrashError> {
        if frame.len() < HEADER_LEN {
            return Err(CrashError::MalformedHeader);
        }
        if frame[0..4] != MAGIC {
            return Err(CrashError::MalformedHeader);
        }
        if frame[4] != VERSION {
            return Err(CrashError::UnsupportedVersion);
        }

        let build_id_len = frame[5] as usize;
        let name_len = frame[6] as usize;
        let pc_count = frame[7] as usize;
        let stack_len = u16::from_le_bytes([frame[8], frame[9]]) as usize;
        let code_len = u16::from_le_bytes([frame[10], frame[11]]) as usize;
        let pid = u32::from_le_bytes([frame[12], frame[13], frame[14], frame[15]]);
        let code = i32::from_le_bytes([frame[16], frame[17], frame[18], frame[19]]);
        let timestamp_nsec = u64::from_le_bytes([
            frame[20], frame[21], frame[22], frame[23], frame[24], frame[25], frame[26], frame[27],
        ]);
        let total_len = u16::from_le_bytes([frame[30], frame[31]]) as usize;

        if total_len != frame.len() || total_len > MAX_TOTAL_FRAME {
            return Err(CrashError::OversizeTotalFrame);
        }
        if build_id_len == 0 || build_id_len > MAX_BUILD_ID_LEN {
            return Err(CrashError::MalformedHeader);
        }
        if name_len == 0 || name_len > MAX_NAME_LEN {
            return Err(CrashError::MalformedHeader);
        }
        if pc_count > MAX_PC_COUNT {
            return Err(CrashError::MalformedHeader);
        }
        if stack_len > MAX_STACK_PREVIEW {
            return Err(CrashError::OversizeStackPreview);
        }
        if code_len > MAX_CODE_PREVIEW {
            return Err(CrashError::OversizeCodePreview);
        }

        let pcs_bytes = pc_count * 8;
        let expected = HEADER_LEN + build_id_len + name_len + pcs_bytes + stack_len + code_len;
        if expected != frame.len() {
            return Err(CrashError::TruncatedFrame);
        }

        let mut off = HEADER_LEN;
        let build_id =
            core::str::from_utf8(&frame[off..off + build_id_len]).map_err(|_| CrashError::MalformedHeader)?;
        off += build_id_len;
        let name =
            core::str::from_utf8(&frame[off..off + name_len]).map_err(|_| CrashError::MalformedHeader)?;
        off += name_len;

        let mut pcs = Vec::with_capacity(pc_count);
        for _ in 0..pc_count {
            let pc = u64::from_le_bytes([
                frame[off],
                frame[off + 1],
                frame[off + 2],
                frame[off + 3],
                frame[off + 4],
                frame[off + 5],
                frame[off + 6],
                frame[off + 7],
            ]);
            pcs.push(pc);
            off += 8;
        }
        let stack_preview = frame[off..off + stack_len].to_vec();
        off += stack_len;
        let code_preview = frame[off..off + code_len].to_vec();

        let out = Self {
            timestamp_nsec,
            pid,
            code,
            name: String::from(name),
            build_id: String::from(build_id),
            pcs,
            stack_preview,
            code_preview,
        };
        out.validate()?;
        Ok(out)
    }
}

#[must_use]
pub fn deterministic_build_id(name: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in name.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    let mut out = String::from("b");
    append_hex_u64(&mut out, h);
    out
}

#[must_use]
pub fn normalize_dump_path(timestamp_nsec: u64, pid: u32, name: &str) -> Result<String, CrashError> {
    let mut sanitized = String::with_capacity(name.len());
    for b in name.bytes() {
        let keep = matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'-' | b'_');
        sanitized.push(if keep { b as char } else { '_' });
    }
    if sanitized.is_empty() {
        sanitized.push_str("unknown");
    }
    let mut out = String::from("/state/crash/");
    append_u64(&mut out, timestamp_nsec);
    out.push('.');
    append_u32(&mut out, pid);
    out.push('.');
    out.push_str(&sanitized);
    out.push_str(".nmd");
    validate_dump_path(&out)?;
    Ok(out)
}

#[must_use]
pub fn validate_dump_path(path: &str) -> Result<(), CrashError> {
    if !path.starts_with("/state/crash/") {
        return Err(CrashError::PathOutOfScope);
    }
    if path.contains("/../")
        || path.contains("/./")
        || path.ends_with("/..")
        || path.ends_with("/.")
        || path.contains("//")
    {
        return Err(CrashError::InvalidPathEscape);
    }
    Ok(())
}

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
pub fn write_dump_to_statefs(path: &str, bytes: &[u8]) -> Result<(), CrashError> {
    validate_dump_path(path)?;
    let client = statefs::client::StatefsClient::new().map_err(|_| CrashError::StatefsWriteFailed)?;
    client.put(path, bytes).map_err(|_| CrashError::StatefsWriteFailed)?;
    client.sync().map_err(|_| CrashError::StatefsWriteFailed)?;
    Ok(())
}

#[cfg(all(feature = "os-lite", nexus_env = "os"))]
pub fn read_dump_from_statefs(path: &str) -> Result<Vec<u8>, CrashError> {
    validate_dump_path(path)?;
    let client = statefs::client::StatefsClient::new().map_err(|_| CrashError::StatefsReadFailed)?;
    client.get(path).map_err(|_| CrashError::StatefsReadFailed)
}

fn append_u32(out: &mut String, mut value: u32) {
    let mut tmp = [0u8; 10];
    let mut i = tmp.len();
    if value == 0 {
        out.push('0');
        return;
    }
    while value != 0 && i != 0 {
        i -= 1;
        tmp[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    for &b in &tmp[i..] {
        out.push(b as char);
    }
}

fn append_u64(out: &mut String, mut value: u64) {
    let mut tmp = [0u8; 20];
    let mut i = tmp.len();
    if value == 0 {
        out.push('0');
        return;
    }
    while value != 0 && i != 0 {
        i -= 1;
        tmp[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    for &b in &tmp[i..] {
        out.push(b as char);
    }
}

fn append_hex_u64(out: &mut String, value: u64) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0xf) as usize;
        out.push(HEX[nibble] as char);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_frame() -> MinidumpFrame {
        MinidumpFrame {
            timestamp_nsec: 1234,
            pid: 7,
            code: 42,
            name: String::from("demo.exit42"),
            build_id: deterministic_build_id("demo.exit42"),
            pcs: vec![0x10, 0x20, 0x30],
            stack_preview: vec![0xAA; 64],
            code_preview: vec![0xCC; 16],
        }
    }

    #[test]
    fn test_minidump_roundtrip() {
        let f = sample_frame();
        let bytes = f.encode().expect("encode");
        let got = MinidumpFrame::decode(&bytes).expect("decode");
        assert_eq!(got, f);
    }

    #[test]
    fn test_normalize_dump_path() {
        let p = normalize_dump_path(99, 12, "demo.exit42").expect("path");
        assert_eq!(p, "/state/crash/99.12.demo.exit42.nmd");
    }

    #[test]
    fn test_normalize_dump_path_is_deterministic() {
        let a = normalize_dump_path(123, 9, "demo/../exit42").expect("path");
        let b = normalize_dump_path(123, 9, "demo/../exit42").expect("path");
        assert_eq!(a, "/state/crash/123.9.demo_.._exit42.nmd");
        assert_eq!(a, b);
    }

    #[test]
    fn test_reject_oversize_minidump_stack_preview() {
        let mut f = sample_frame();
        f.stack_preview = vec![0u8; MAX_STACK_PREVIEW + 1];
        assert_eq!(f.encode(), Err(CrashError::OversizeStackPreview));
    }

    #[test]
    fn test_reject_oversize_minidump_total_frame() {
        let mut f = sample_frame();
        f.pcs = vec![0xFF; MAX_PC_COUNT + 1];
        assert_eq!(f.encode(), Err(CrashError::OversizeTotalFrame));
    }

    #[test]
    fn test_reject_invalid_crash_dump_path_escape() {
        assert_eq!(
            validate_dump_path("/state/crash/../../etc/passwd"),
            Err(CrashError::InvalidPathEscape)
        );
        assert_eq!(validate_dump_path("/tmp/crash/x.nmd"), Err(CrashError::PathOutOfScope));
    }

    #[test]
    fn test_reject_malformed_minidump_header() {
        let bad = vec![0u8; HEADER_LEN];
        assert_eq!(MinidumpFrame::decode(&bad), Err(CrashError::MalformedHeader));

        let mut short = sample_frame().encode().expect("encode");
        short[0] = b'X';
        assert_eq!(MinidumpFrame::decode(&short), Err(CrashError::MalformedHeader));
    }

    #[test]
    fn test_minidump_frame_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MinidumpFrame>();
    }
}
