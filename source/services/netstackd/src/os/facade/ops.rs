// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Shared facade operation helpers for netstackd dispatch
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[cfg(test)]
use super::validation::StepOutcome;

#[cfg(test)]
#[must_use]
#[inline]
pub(crate) fn pending_connect_ready(is_ready: bool) -> StepOutcome {
    if is_ready {
        StepOutcome::Ready
    } else {
        StepOutcome::WouldBlock
    }
}

#[must_use]
#[inline]
pub(crate) fn is_unexpected_pending_connect_state(
    reused_pending: bool,
    pending_slot_empty: bool,
) -> bool {
    reused_pending && pending_slot_empty
}
