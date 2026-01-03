// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Versioned discovery announce packet (v1) for DSoftBus OS transport
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 5 integration tests (golden vector + negative decode cases)
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![forbid(unsafe_code)]

use thiserror::Error;

/// Discovery announce packet v1.
///
/// This is a host-first, deterministic seed for the OS UDP discovery payload. It is bounded and
/// versioned to prevent silent drift.
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

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PacketError {
    #[error("truncated")]
    Truncated,
    #[error("bad magic")]
    BadMagic,
    #[error("unsupported version {0}")]
    UnsupportedVersion(u8),
    #[error("invalid input: {0}")]
    InvalidInput(&'static str),
    #[error("utf8 error")]
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
    let device_id = core::str::from_utf8(dev)
        .map_err(|_| PacketError::Utf8)?
        .to_string();

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
        let s = core::str::from_utf8(s).map_err(|_| PacketError::Utf8)?.to_string();
        services.push(s);
    }

    Ok(AnnounceV1 {
        device_id,
        port,
        noise_static,
        services,
    })
}

