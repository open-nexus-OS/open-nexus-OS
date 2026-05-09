// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host-testable scanout state machine for service-owned display refresh.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `fbdevd` host reject tests.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

use alloc::{format, string::String};

use windowd::DisplayPresentHandoff;

use crate::error::{FbdevdError, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayScanout {
    configured: bool,
    last_generation: u64,
    last_report_ns: u64,
    flush_events: u64,
    vsync_events: u64,
    flush_failures: u64,
    stale_scanout: u64,
    bytes_flushed: u64,
}

impl DisplayScanout {
    pub fn new() -> Self {
        Self {
            configured: false,
            last_generation: 0,
            last_report_ns: 0,
            flush_events: 0,
            vsync_events: 0,
            flush_failures: 0,
            stale_scanout: 0,
            bytes_flushed: 0,
        }
    }

    pub fn configure(&mut self) {
        self.configured = true;
    }

    pub fn present(&mut self, generation: u64, handoff: &DisplayPresentHandoff) -> Result<u64> {
        if !self.configured {
            self.flush_failures = self.flush_failures.saturating_add(1);
            return Err(FbdevdError::FlushWithoutConfiguredBackend);
        }
        let byte_len = handoff
            .byte_len()
            .map_err(|_| FbdevdError::PresentWithoutFrame)?;
        if byte_len == 0 {
            self.flush_failures = self.flush_failures.saturating_add(1);
            return Err(FbdevdError::PresentWithoutFrame);
        }
        self.present_bytes(generation, byte_len)
    }

    pub fn present_bytes(&mut self, generation: u64, byte_len: usize) -> Result<u64> {
        if !self.configured {
            self.flush_failures = self.flush_failures.saturating_add(1);
            return Err(FbdevdError::FlushWithoutConfiguredBackend);
        }
        if byte_len == 0 {
            self.flush_failures = self.flush_failures.saturating_add(1);
            return Err(FbdevdError::PresentWithoutFrame);
        }
        if generation <= self.last_generation {
            self.stale_scanout = self.stale_scanout.saturating_add(1);
            return Err(FbdevdError::StaleScanoutGeneration);
        }
        self.last_generation = generation;
        self.flush_events = self.flush_events.saturating_add(1);
        self.vsync_events = self.vsync_events.saturating_add(1);
        self.bytes_flushed = self.bytes_flushed.saturating_add(byte_len as u64);
        Ok(self.last_generation)
    }

    pub fn report_if_due(&mut self, now_ns: u64) -> Option<String> {
        if now_ns == 0 {
            return None;
        }
        if self.last_report_ns == 0 {
            self.last_report_ns = now_ns;
            return None;
        }
        let elapsed = now_ns.saturating_sub(self.last_report_ns);
        if elapsed < 1_000_000_000 {
            return None;
        }
        let flush_hz = self
            .flush_events
            .saturating_mul(1_000_000_000)
            .checked_div(elapsed)
            .unwrap_or(0);
        let vsync_hz = self
            .vsync_events
            .saturating_mul(1_000_000_000)
            .checked_div(elapsed)
            .unwrap_or(0);
        let line = format!(
            "fps: fbdevd flush_hz={} vsync_hz={} bytes={} flush_fail={} stale_scanout={}",
            flush_hz, vsync_hz, self.bytes_flushed, self.flush_failures, self.stale_scanout
        );
        self.last_report_ns = now_ns;
        self.flush_events = 0;
        self.vsync_events = 0;
        self.flush_failures = 0;
        self.stale_scanout = 0;
        self.bytes_flushed = 0;
        Some(line)
    }
}
