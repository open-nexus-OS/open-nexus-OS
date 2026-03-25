// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TCP-specific bounded helpers for netstackd facade operations
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Planned in netstackd host seam tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[cfg(test)]
use super::validation::StepOutcome;

#[cfg(not(test))]
use nexus_abi::yield_;
#[cfg(not(test))]
use nexus_net::{NetError, NetStack as _};
#[cfg(not(test))]
use nexus_net_os::SmoltcpVirtioNetStack;

#[cfg(not(test))]
use crate::os::config::{TCP_READY_SPIN_BUDGET, TCP_READY_STEP_MS};

#[cfg(test)]
#[must_use = "must evaluate writable wait outcome before deciding retry behavior"]
#[inline]
pub(crate) fn wait_writable_outcome(ready: bool) -> StepOutcome {
    if ready {
        StepOutcome::Ready
    } else {
        StepOutcome::WouldBlock
    }
}

#[cfg(not(test))]
pub(crate) fn retry_would_block<T, F>(
    net: &mut SmoltcpVirtioNetStack,
    now_ms: u64,
    mut op: F,
) -> Result<T, NetError>
where
    F: FnMut(u64) -> Result<T, NetError>,
{
    let mut result = op(now_ms.saturating_add(TCP_READY_STEP_MS));
    if matches!(result, Err(NetError::WouldBlock)) {
        for _ in 0..TCP_READY_SPIN_BUDGET {
            let tick = (nexus_abi::nsec().unwrap_or(0) / 1_000_000) as u64;
            net.poll(tick);
            let _ = yield_();
            result = op(tick.saturating_add(TCP_READY_STEP_MS));
            if !matches!(result, Err(NetError::WouldBlock)) {
                break;
            }
        }
    }
    result
}
