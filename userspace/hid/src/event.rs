// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Typed HID event records and newtypes for deterministic host input parsing.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: No direct tests (covered by 5 integration tests in `tests/input_v1_0_host/tests/hid_contract.rs`).
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimestampNs(u64);

impl TimestampNs {
    #[must_use]
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HidCode(u16);

impl HidCode {
    #[must_use]
    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HidValue(i32);

impl HidValue {
    #[must_use]
    pub const fn new(raw: i32) -> Self {
        Self(raw)
    }

    #[must_use]
    pub const fn raw(self) -> i32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidEventKind {
    Key,
    Rel,
    Btn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HidEvent {
    timestamp: TimestampNs,
    kind: HidEventKind,
    code: HidCode,
    value: HidValue,
}

impl HidEvent {
    #[must_use]
    pub const fn new(
        timestamp: TimestampNs,
        kind: HidEventKind,
        code: HidCode,
        value: HidValue,
    ) -> Self {
        Self { timestamp, kind, code, value }
    }

    #[must_use]
    pub const fn key(timestamp: TimestampNs, code: u16, value: i32) -> Self {
        Self::new(timestamp, HidEventKind::Key, HidCode::new(code), HidValue::new(value))
    }

    #[must_use]
    pub const fn rel(timestamp: TimestampNs, code: u16, value: i32) -> Self {
        Self::new(timestamp, HidEventKind::Rel, HidCode::new(code), HidValue::new(value))
    }

    #[must_use]
    pub const fn btn(timestamp: TimestampNs, code: u16, value: i32) -> Self {
        Self::new(timestamp, HidEventKind::Btn, HidCode::new(code), HidValue::new(value))
    }

    #[must_use]
    pub const fn timestamp(self) -> TimestampNs {
        self.timestamp
    }

    #[must_use]
    pub const fn kind(self) -> HidEventKind {
        self.kind
    }

    #[must_use]
    pub const fn code(self) -> HidCode {
        self.code
    }

    #[must_use]
    pub const fn value(self) -> HidValue {
        self.value
    }
}
