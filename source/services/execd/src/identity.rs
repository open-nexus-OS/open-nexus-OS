// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the RFC-0086 app-host service-name derivation — `app:<bundle_id>`,
//! the kernel-attributed identity every app-host child carries via `exec_v2`.
//! Pure and host-tested (`os_lite.rs` is RISC-V-only); execd is the ONLY
//! naming authority, and the `app:` prefix keeps bundle ids out of the boot
//! service namespace by construction.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (derivation, bounds, rejects).

/// Maximum bundle-id length accepted on the spawn wire (`app_len ≤ 48`),
/// which keeps the derived name within the kernel's 64-byte service-name
/// bound with room to spare.
const MAX_APP_ID: usize = 48;

/// The derived service name, a fixed-capacity string (`no_std`, no alloc).
pub(crate) struct AppServiceName {
    buf: [u8; 4 + MAX_APP_ID],
    len: usize,
}

impl AppServiceName {
    pub(crate) fn as_str(&self) -> &str {
        // Constructed only from validated UTF-8 (`app_service_name`), and
        // `app:` is ASCII — this cannot fail; the fallback keeps the
        // no-unwrap rule on what is ultimately wire-derived data.
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("app:")
    }
}

/// `app:<bundle_id>` for a wire-supplied bundle id, or `None` when the id is
/// empty, oversized, or not UTF-8 (the caller then spawns NAMELESS — sid 0
/// has no window authority anywhere, so a bad id degrades closed).
pub(crate) fn app_service_name(app_id: &[u8]) -> Option<AppServiceName> {
    if app_id.is_empty() || app_id.len() > MAX_APP_ID {
        return None;
    }
    if core::str::from_utf8(app_id).is_err() {
        return None;
    }
    let mut buf = [0u8; 4 + MAX_APP_ID];
    buf[..4].copy_from_slice(b"app:");
    buf[4..4 + app_id.len()].copy_from_slice(app_id);
    Some(AppServiceName { buf, len: 4 + app_id.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_prefixed_name() {
        let n = app_service_name(b"calculator").expect("derives");
        assert_eq!(n.as_str(), "app:calculator");
    }

    #[test]
    fn rejects_empty_and_oversized() {
        assert!(app_service_name(b"").is_none());
        assert!(app_service_name(&[b'a'; 49]).is_none());
        assert!(app_service_name(&[b'a'; 48]).is_some(), "48 is the wire bound, allowed");
    }

    #[test]
    fn rejects_non_utf8() {
        assert!(app_service_name(&[0xFF, 0xFE]).is_none());
    }

    #[test]
    fn name_stays_within_kernel_bound() {
        let n = app_service_name(&[b'x'; 48]).expect("derives");
        assert!(n.as_str().len() <= 64, "kernel service-name bound");
    }
}
