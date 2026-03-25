// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Typed handle wrappers for netstackd listener, stream, and udp IDs
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ListenerId(usize);

impl ListenerId {
    #[inline]
    pub(crate) fn from_wire(raw: u32) -> Option<Self> {
        if raw == 0 {
            None
        } else {
            Some(Self((raw - 1) as usize))
        }
    }

    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0
    }

    #[inline]
    pub(crate) fn to_wire(index: usize) -> u32 {
        (index + 1) as u32
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StreamId(usize);

impl StreamId {
    #[inline]
    pub(crate) const fn from_index(index: usize) -> Self {
        Self(index)
    }

    #[inline]
    pub(crate) fn from_wire(raw: u32) -> Option<Self> {
        if raw == 0 {
            None
        } else {
            Some(Self((raw - 1) as usize))
        }
    }

    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0
    }

    #[inline]
    pub(crate) fn to_wire(index: usize) -> u32 {
        (index + 1) as u32
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UdpId(usize);

impl UdpId {
    #[inline]
    pub(crate) fn from_wire(raw: u32) -> Option<Self> {
        if raw == 0 {
            None
        } else {
            Some(Self((raw - 1) as usize))
        }
    }

    #[inline]
    pub(crate) fn index(self) -> usize {
        self.0
    }

    #[inline]
    pub(crate) fn to_wire(index: usize) -> u32 {
        (index + 1) as u32
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ReplyCapSlot(u32);

impl ReplyCapSlot {
    #[inline]
    pub(crate) fn new(raw: u32) -> Self {
        Self(raw)
    }

    #[inline]
    pub(crate) fn raw(self) -> u32 {
        self.0
    }
}
