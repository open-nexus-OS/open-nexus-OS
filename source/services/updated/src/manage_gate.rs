// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `updates.manage` gate (TASK-0140, RFC-0089 §8) — pure decision
//! logic, cfg-free so the host suite proves the exact mapping the OS
//! request loop executes. Mutating ops (stage/switch/rollback) require a
//! policyd grant of `updates.manage` on the KERNEL-ATTRIBUTED sender;
//! deny-by-default: an unreachable policyd denies. Reads (status/feed/
//! check) and the boot-spine pass-throughs (health/boot-attempt, quorum-
//! and allowlist-gated in bootctld) stay open — the bootctld "reads for
//! anyone" rule.
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (`test_reject_*`)
//! ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md

use nexus_wire::updated::{OP_ROLLBACK, OP_STAGE_SOURCE, OP_SWITCH};

/// The delegated policyd verdict as the request loop sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyAnswer {
    Allow,
    Deny,
    /// Transport trouble reaching policyd — MUST deny (fail closed).
    Unreachable,
}

/// The capability mutating ops require.
pub const MANAGE_CAP: &[u8] = b"updates.manage";

/// `true` for the ops that change slot/stage state and therefore carry
/// the `updates.manage` gate.
#[must_use]
pub fn is_mutating(op: u8) -> bool {
    matches!(op, OP_STAGE_SOURCE | OP_SWITCH | OP_ROLLBACK)
}

/// The gate: reads pass unconditionally; mutating ops pass only on an
/// explicit policyd Allow.
#[must_use]
pub fn allows(op: u8, policy: PolicyAnswer) -> bool {
    if !is_mutating(op) {
        return true;
    }
    policy == PolicyAnswer::Allow
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_wire::updated::{
        OP_BOOT_ATTEMPT, OP_CHECK, OP_FEED_LIST, OP_GET_STATUS, OP_HEALTH_OK,
    };

    #[test]
    fn test_reject_mutating_without_grant() {
        for op in [OP_STAGE_SOURCE, OP_SWITCH, OP_ROLLBACK] {
            assert!(!allows(op, PolicyAnswer::Deny), "op {op} must deny without the grant");
        }
    }

    #[test]
    fn test_reject_mutating_when_policyd_unreachable() {
        // Deny-by-default: transport trouble is never an allow.
        for op in [OP_STAGE_SOURCE, OP_SWITCH, OP_ROLLBACK] {
            assert!(!allows(op, PolicyAnswer::Unreachable), "op {op} must fail closed");
        }
    }

    #[test]
    fn mutating_ops_pass_with_the_grant() {
        for op in [OP_STAGE_SOURCE, OP_SWITCH, OP_ROLLBACK] {
            assert!(allows(op, PolicyAnswer::Allow));
        }
    }

    #[test]
    fn reads_and_boot_spine_stay_open() {
        // Reads follow bootctld's "reads for anyone"; health/boot-attempt
        // keep their own gates in bootctld (quorum membership, allowlist).
        for op in [OP_GET_STATUS, OP_FEED_LIST, OP_CHECK, OP_HEALTH_OK, OP_BOOT_ATTEMPT] {
            assert!(!is_mutating(op));
            for policy in [PolicyAnswer::Allow, PolicyAnswer::Deny, PolicyAnswer::Unreachable] {
                assert!(allows(op, policy));
            }
        }
    }
}
