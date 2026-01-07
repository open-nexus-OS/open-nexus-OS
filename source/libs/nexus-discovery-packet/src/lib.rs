// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Versioned discovery announce packet (v1) for DSoftBus OS transport (no_std)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable (Phase 1; stable framing in Phase 2)
//! TEST_COVERAGE: 5 integration tests (golden vector + negative decode cases)
//! ADR: docs/adr/0005-dsoftbus-architecture.md
//! RFC: docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md
//!
//! Notes:
//! - This is a **no_std** port of `userspace/dsoftbus/src/discovery_packet.rs` for OS-side use.
//! - It provides bounded, versioned UDP discovery packet parsing/encoding for DSoftBus OS transport.
//! - Packet format: MAGIC (4 bytes) + VERSION (1 byte) + device_id (length-prefixed UTF-8) + port (2 bytes BE) + noise_static (32 bytes) + services (count + length-prefixed UTF-8 strings)

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::str;

/// Discovery announce packet v1.
///
/// This is a bounded and versioned packet for OS UDP discovery payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnnounceV1 {
    /// Stable device identifier string (UTF-8).
    pub device_id: String,
    /// Listening port for session establishment.
    pub port: u16,
    /// Noise static public key bytes (X25519).
    pub noise_static: [u8; 32],
    /// Published service names (UTF-8).
    pub services: Vec<String>,
}

pub const MAGIC: [u8; 4] = *b"NXSB";
pub const VERSION_V1: u8 = 1;

pub const MAX_DEVICE_ID_BYTES: usize = 64;
pub const MAX_SERVICE_NAME_BYTES: usize = 64;
pub const MAX_SERVICE_COUNT: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PacketError {
    Truncated,
    BadMagic,
    UnsupportedVersion(u8),
    InvalidInput(&'static str),
    Utf8,
}

fn take<'a>(buf: &mut &'a [u8], n: usize) -> Result<&'a [u8], PacketError> {
    if buf.len() < n {
        return Err(PacketError::Truncated);
    }
    let (a, b) = buf.split_at(n);
    *buf = b;
    Ok(a)
}

pub fn encode_announce_v1(pkt: &AnnounceV1) -> Result<Vec<u8>, PacketError> {
    let dev = pkt.device_id.as_bytes();
    if dev.is_empty() || dev.len() > MAX_DEVICE_ID_BYTES {
        return Err(PacketError::InvalidInput("device_id length"));
    }
    if pkt.services.len() > MAX_SERVICE_COUNT {
        return Err(PacketError::InvalidInput("service count"));
    }
    for s in &pkt.services {
        let b = s.as_bytes();
        if b.is_empty() || b.len() > MAX_SERVICE_NAME_BYTES {
            return Err(PacketError::InvalidInput("service name length"));
        }
    }

    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.push(VERSION_V1);
    out.push(dev.len() as u8);
    out.extend_from_slice(dev);
    out.extend_from_slice(&pkt.port.to_be_bytes());
    out.extend_from_slice(&pkt.noise_static);
    out.push(pkt.services.len() as u8);
    for s in &pkt.services {
        let b = s.as_bytes();
        out.push(b.len() as u8);
        out.extend_from_slice(b);
    }
    Ok(out)
}

pub fn decode_announce_v1(bytes: &[u8]) -> Result<AnnounceV1, PacketError> {
    let mut b = bytes;
    let magic = take(&mut b, 4)?;
    if magic != MAGIC {
        return Err(PacketError::BadMagic);
    }
    let ver = take(&mut b, 1)?[0];
    if ver != VERSION_V1 {
        return Err(PacketError::UnsupportedVersion(ver));
    }

    let dev_len = take(&mut b, 1)?[0] as usize;
    if dev_len == 0 || dev_len > MAX_DEVICE_ID_BYTES {
        return Err(PacketError::InvalidInput("device_id length"));
    }
    let dev = take(&mut b, dev_len)?;
    let device_id = str::from_utf8(dev).map_err(|_| PacketError::Utf8)?.to_string();

    let port_bytes = take(&mut b, 2)?;
    let port = u16::from_be_bytes([port_bytes[0], port_bytes[1]]);

    let ns = take(&mut b, 32)?;
    let mut noise_static = [0u8; 32];
    noise_static.copy_from_slice(ns);

    let svc_count = take(&mut b, 1)?[0] as usize;
    if svc_count > MAX_SERVICE_COUNT {
        return Err(PacketError::InvalidInput("service count"));
    }
    let mut services = Vec::with_capacity(svc_count);
    for _ in 0..svc_count {
        let n = take(&mut b, 1)?[0] as usize;
        if n == 0 || n > MAX_SERVICE_NAME_BYTES {
            return Err(PacketError::InvalidInput("service name length"));
        }
        let s = take(&mut b, n)?;
        let s = str::from_utf8(s).map_err(|_| PacketError::Utf8)?.to_string();
        services.push(s);
    }

    Ok(AnnounceV1 { device_id, port, noise_static, services })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announce_v1_golden_vector_bytes() {
        let pkt = AnnounceV1 {
            device_id: "node9".to_string(),
            port: 37020,
            noise_static: [0x11; 32],
            services: alloc::vec!["samgrd".into(), "bundlemgrd".into()],
        };

        let bytes = encode_announce_v1(&pkt).expect("encode");

        // MAGIC "NXSB", ver=1, dev_len=5, "node9", port=37020 (0x909C), noise_static, svc_count=2,
        // "samgrd", "bundlemgrd"
        let mut expected = Vec::new();
        expected.extend_from_slice(b"NXSB");
        expected.push(1);
        expected.push(5);
        expected.extend_from_slice(b"node9");
        expected.extend_from_slice(&37020u16.to_be_bytes());
        expected.extend_from_slice(&[0x11; 32]);
        expected.push(2);
        expected.push(b"samgrd".len() as u8);
        expected.extend_from_slice(b"samgrd");
        expected.push(b"bundlemgrd".len() as u8);
        expected.extend_from_slice(b"bundlemgrd");

        assert_eq!(bytes, expected, "announce v1 bytes drifted");

        let decoded = decode_announce_v1(&bytes).expect("decode");
        assert_eq!(decoded, pkt);
    }

    #[test]
    fn announce_v1_rejects_overlong_service_name() {
        let pkt = AnnounceV1 {
            device_id: "node9".to_string(),
            port: 1,
            noise_static: [0; 32],
            services: alloc::vec!["a".repeat(100)],
        };
        let err = encode_announce_v1(&pkt).expect_err("expected bounds error");
        assert_eq!(err, PacketError::InvalidInput("service name length"));
    }

    #[test]
    fn announce_v1_rejects_bad_magic() {
        let bytes = b"NOPE";
        assert_eq!(decode_announce_v1(bytes), Err(PacketError::BadMagic));
    }

    #[test]
    fn announce_v1_rejects_unsupported_version() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NXSB");
        bytes.push(99); // unsupported version
        assert_eq!(decode_announce_v1(&bytes), Err(PacketError::UnsupportedVersion(99)));
    }

    #[test]
    fn announce_v1_rejects_truncated() {
        let bytes = b"NXS"; // truncated magic
        assert_eq!(decode_announce_v1(bytes), Err(PacketError::Truncated));
    }
}
