// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Newtypes for caller, surface, commit, present, frame, fence, input, and VMO handle identity.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Newtype behavior covered indirectly by `ui_windowd_host` and `ui_v2a_host`
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

const SYSTEM_CALLER_RAW: u64 = 1;

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CallerId(u64);

impl CallerId {
    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn system() -> Self {
        Self(SYSTEM_CALLER_RAW)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallerCtx {
    caller_id: CallerId,
}

impl CallerCtx {
    pub const fn from_service_metadata(service_id: u64) -> Self {
        Self { caller_id: CallerId(service_id) }
    }

    pub const fn system() -> Self {
        Self { caller_id: CallerId::system() }
    }

    pub const fn caller_id(self) -> CallerId {
        self.caller_id
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SurfaceId(u64);

impl SurfaceId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CommitSeq(u64);

impl CommitSeq {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PresentSeq(u64);

impl PresentSeq {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FrameIndex(u64);

impl FrameIndex {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FenceId(u64);

impl FenceId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct InputSeq(u64);

impl InputSeq {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct VmoHandleId(u64);

impl VmoHandleId {
    pub const fn new(raw: u64) -> Self {
        Self(raw)
    }
}
