// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: logd sink hardening guards for identity/payload/rate limits
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: Host tests in `source/services/logd/tests/journal_protocol.rs`
//! ADR: docs/adr/0017-service-architecture.md
//!
//! INVARIANTS:
//! - Identity/policy decisions bind to kernel sender metadata, never payload claims
//! - APPEND rate limiting is deterministic and bounded per sender
//! - Guard state is bounded (no unbounded sender table growth)

#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec::Vec;

/// Hard cap for tracked sender subjects in rate limiting state.
pub const MAX_RATE_SUBJECTS: usize = 64;
/// Fixed token bucket window length.
pub const RATE_WINDOW_NS: u64 = 1_000_000_000;
/// Maximum accepted APPEND operations per sender per window.
pub const RATE_MAX_APPENDS_PER_WINDOW: u32 = 24;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SenderWindow {
    sender_service_id: u64,
    window_start_ns: u64,
    used_in_window: u32,
}

/// Deterministic per-sender APPEND limiter.
pub struct SenderRateLimiter {
    entries: Vec<SenderWindow>,
}

impl SenderRateLimiter {
    /// Create an empty bounded limiter.
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// Returns `true` when the sender is over budget (request must be rejected).
    pub fn is_rate_limited(&mut self, sender_service_id: u64, now_nsec: u64) -> bool {
        if let Some(pos) =
            self.entries.iter().position(|entry| entry.sender_service_id == sender_service_id)
        {
            let entry = &mut self.entries[pos];
            if now_nsec.saturating_sub(entry.window_start_ns) >= RATE_WINDOW_NS {
                entry.window_start_ns = now_nsec;
                entry.used_in_window = 0;
            }
            if entry.used_in_window >= RATE_MAX_APPENDS_PER_WINDOW {
                return true;
            }
            entry.used_in_window = entry.used_in_window.saturating_add(1);
            return false;
        }

        // Bounded state: unknown senders beyond cap are deterministically rejected.
        if self.entries.len() >= MAX_RATE_SUBJECTS {
            return true;
        }
        self.entries.push(SenderWindow {
            sender_service_id,
            window_start_ns: now_nsec,
            used_in_window: 1,
        });
        false
    }
}

impl Default for SenderRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns `true` if APPEND fields contain payload identity claims that must not be trusted.
///
/// Fields format is the RFC-0011 convention: `key=value\n` (sorted by key).
pub fn has_payload_identity_claim(fields: &[u8]) -> bool {
    let mut start = 0usize;
    while start < fields.len() {
        let mut end = start;
        while end < fields.len() && fields[end] != b'\n' {
            end += 1;
        }
        let line = &fields[start..end];
        if let Some(eq_pos) = line.iter().position(|b| *b == b'=') {
            let key = &line[..eq_pos];
            if is_forbidden_identity_key(key) {
                return true;
            }
        }
        start = end.saturating_add(1);
    }
    false
}

fn is_forbidden_identity_key(key: &[u8]) -> bool {
    key == b"sender_service_id"
        || key == b"service_id"
        || key == b"requester_id"
        || key == b"service"
}
