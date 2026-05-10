// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Deterministic marker and log formatting helpers for netstackd
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[cfg(all(
    nexus_env = "os",
    target_arch = "riscv64",
    target_os = "none",
    feature = "os-lite"
))]
use nexus_net_os::DhcpConfig;

#[inline]
pub(crate) fn write_u8(val: u8, out: &mut [u8]) -> usize {
    if val >= 100 {
        out[0] = b'0' + (val / 100);
        out[1] = b'0' + ((val / 10) % 10);
        out[2] = b'0' + (val % 10);
        3
    } else if val >= 10 {
        out[0] = b'0' + (val / 10);
        out[1] = b'0' + (val % 10);
        2
    } else {
        out[0] = b'0' + val;
        1
    }
}

#[inline]
pub(crate) fn write_ip(ip: &[u8; 4], out: &mut [u8]) -> usize {
    let mut pos = 0;
    for (i, octet) in ip.iter().enumerate() {
        if i > 0 {
            out[pos] = b'.';
            pos += 1;
        }
        pos += write_u8(*octet, &mut out[pos..]);
    }
    pos
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
pub(crate) fn emit_dhcp_bound_marker(config: &DhcpConfig) {
    let mut buf = [0u8; 64];
    let mut pos = 0;
    let prefix = b"net: dhcp bound ";
    buf[pos..pos + prefix.len()].copy_from_slice(prefix);
    pos += prefix.len();
    pos += write_ip(&config.ip, &mut buf[pos..]);
    buf[pos] = b'/';
    pos += 1;
    pos += write_u8(config.prefix_len, &mut buf[pos..]);
    let gw_prefix = b" gw=";
    buf[pos..pos + gw_prefix.len()].copy_from_slice(gw_prefix);
    pos += gw_prefix.len();
    if let Some(gw) = config.gateway {
        pos += write_ip(&gw, &mut buf[pos..]);
    } else {
        let none = b"none";
        buf[pos..pos + none.len()].copy_from_slice(none);
        pos += none.len();
    }
    if let Ok(s) = core::str::from_utf8(&buf[..pos]) {
        let _ = nexus_abi::debug_println(s);
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
pub(crate) fn emit_smoltcp_iface_marker(config: &DhcpConfig) {
    let mut buf = [0u8; 48];
    let mut pos = 0;
    let prefix = b"net: smoltcp iface up ";
    buf[pos..pos + prefix.len()].copy_from_slice(prefix);
    pos += prefix.len();
    pos += write_ip(&config.ip, &mut buf[pos..]);
    if let Ok(s) = core::str::from_utf8(&buf[..pos]) {
        let _ = nexus_abi::debug_println(s);
    }
}

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none", feature = "os-lite"))]
pub(crate) fn emit_fallback_static_marker(ip: [u8; 4], prefix_len: u8) {
    let mut buf = [0u8; 64];
    let mut pos = 0usize;
    let prefix = b"net: dhcp unavailable (fallback static ";
    buf[pos..pos + prefix.len()].copy_from_slice(prefix);
    pos += prefix.len();
    pos += write_ip(&ip, &mut buf[pos..]);
    buf[pos] = b'/';
    pos += 1;
    pos += write_u8(prefix_len, &mut buf[pos..]);
    buf[pos] = b')';
    pos += 1;
    if let Ok(s) = core::str::from_utf8(&buf[..pos]) {
        let _ = nexus_abi::debug_println(s);
    }
}
