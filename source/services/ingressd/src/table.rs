// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The build-time exposure table (RFC-0092 §1) — `build.rs`
//! compiles every `[[expose]]` of the policy SSOT through the shared
//! `expose.rs` grammar into `generated::EXPOSE_ENTRIES`. Attribution is by
//! subject id (FNV-1a of the canonical name, the one policy hash) and the
//! table is the only source of what the gateway may front.
//! OWNERS: @runtime @security
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: tests/ingress_host/ (`shipped_policy_table_is_consistent`)

/// Transport of an exposure (wire byte = discriminant).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Proto {
    Tcp = 0,
    Udp = 1,
}

impl Proto {
    /// Wire byte → proto; anything else is malformed.
    pub const fn from_wire(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::Tcp),
            1 => Some(Self::Udp),
            _ => None,
        }
    }

    /// Marker/wire label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

/// One IPv4 allow-list entry (canonical host bits, `len ≤ 32`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cidr {
    pub addr: [u8; 4],
    pub len: u8,
}

/// One declared exposure as the gateway sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExposeEntry {
    /// Declared subject; must equal the kernel-attributed sender of the intent.
    pub subject_id: u64,
    pub port: u16,
    pub proto: Proto,
    pub cidr_allow: &'static [Cidr],
    pub rate_per_s: u32,
    pub burst: u32,
    /// Loopback backend port the subject listens on.
    pub backend: u16,
}

/// The shipped table (`policies/*`), generated at build time.
pub mod generated {
    // The generated file names its types by `super::` path so an empty
    // table (no exposure declared) leaves no import unused.
    include!(concat!(env!("OUT_DIR"), "/expose_table.rs"));
}

/// The table entry owning `(port, proto)`, with its index.
pub fn lookup(table: &[ExposeEntry], port: u16, proto: Proto) -> Option<(usize, &ExposeEntry)> {
    table.iter().enumerate().find(|(_, e)| e.port == port && e.proto == proto)
}
