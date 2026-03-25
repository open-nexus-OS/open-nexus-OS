// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared validation and bounded-step helpers for facade operations
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub(crate) enum StepOutcome {
    Ready,
    WouldBlock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub(crate) enum ValidationOutcome {
    Valid,
    Malformed,
}

impl ValidationOutcome {
    #[inline]
    pub(crate) const fn is_valid(self) -> bool {
        matches!(self, Self::Valid)
    }

    #[inline]
    pub(crate) const fn is_malformed(self) -> bool {
        !self.is_valid()
    }
}

#[cfg(test)]
#[inline]
pub(crate) fn validate_exact_len(len: usize, expected: usize) -> ValidationOutcome {
    if len == expected {
        ValidationOutcome::Valid
    } else {
        ValidationOutcome::Malformed
    }
}

#[inline]
pub(crate) fn validate_exact_or_nonce_len(len: usize, base: usize) -> ValidationOutcome {
    if len == base || len == base + 8 {
        ValidationOutcome::Valid
    } else {
        ValidationOutcome::Malformed
    }
}

#[inline]
pub(crate) fn validate_payload_len(
    req_len: usize,
    prefix: usize,
    payload_len: usize,
) -> ValidationOutcome {
    if req_len == prefix + payload_len || req_len == prefix + payload_len + 8 {
        ValidationOutcome::Valid
    } else {
        ValidationOutcome::Malformed
    }
}
