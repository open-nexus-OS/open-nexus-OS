// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Boot stages (ADR-0062, RFC-0093 §3). ONE monotone kernel timeline fence per boot
//! carries the whole boot order: `Platform < DisplayReady < SessionStart < ShellVisible`. A
//! service declares WHICH stage it belongs to (`ServiceSpec.stage`) and waits for that stage's
//! PREREQUISITE before its `entry()` — so bring-up order is a property of declarations, never of
//! resume order, `yield_()` calls or time caps.
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P5-b)
//! API_STABILITY: Stable (the fence values are a boot contract)
//! TEST_COVERAGE: host tests below + `nexus-init`'s barrier tests
//! ADR: docs/adr/0062-boot-stage-fence-and-readiness-barriers.md

/// A boot stage. The discriminant IS the fence value the kernel timeline carries, so the
/// ordering is monotone by construction and a stage can never be "half reached".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Stage {
    /// The platform floor: policy, storage, the block plane, the boot-state authority.
    Platform = 1,
    /// The display chain owns a scanout and windowd has presented its first checked frame.
    DisplayReady = 2,
    /// Boot state is committed and the registry is populated — a session may begin.
    SessionStart = 3,
    /// The shell's first frame is visible.
    ShellVisible = 4,
}

impl Stage {
    /// Every stage, in boot order.
    pub const ALL: [Stage; 4] =
        [Stage::Platform, Stage::DisplayReady, Stage::SessionStart, Stage::ShellVisible];

    /// The fence value this stage signals.
    pub const fn value(self) -> u64 {
        self as u8 as u64
    }

    /// The fence value a member of this stage waits for before `entry()`. `Platform` members
    /// wait for nothing (0 — the fence starts there), which is what makes the platform floor
    /// able to bring itself up.
    pub const fn prerequisite(self) -> u64 {
        match self {
            Stage::Platform => 0,
            Stage::DisplayReady => Stage::Platform.value(),
            Stage::SessionStart => Stage::DisplayReady.value(),
            Stage::ShellVisible => Stage::SessionStart.value(),
        }
    }

    /// Stable marker token (`stage: <label>`), part of the boot-log contract.
    pub const fn label(self) -> &'static str {
        match self {
            Stage::Platform => "platform",
            Stage::DisplayReady => "display-ready",
            Stage::SessionStart => "session-start",
            Stage::ShellVisible => "shell-visible",
        }
    }

    /// Parse a stage from the `@stage <label>` control-channel verb. Unknown labels are
    /// refused (fail closed) — a typo must never silently advance the boot.
    pub fn from_label(label: &[u8]) -> Option<Self> {
        Some(match label {
            b"platform" => Stage::Platform,
            b"display-ready" => Stage::DisplayReady,
            b"session-start" => Stage::SessionStart,
            b"shell-visible" => Stage::ShellVisible,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fence is MONOTONE: a stage's value must exceed its prerequisite, and the ladder must
    /// be strictly increasing. A stage that did not advance the value would be a barrier that
    /// never blocks — silently, for the whole fleet.
    #[test]
    fn test_reject_stage_order_monotonic() {
        let mut previous = 0u64;
        for stage in Stage::ALL {
            assert!(
                stage.value() > previous,
                "{stage:?} does not advance the fence beyond {previous}"
            );
            assert!(
                stage.prerequisite() < stage.value(),
                "{stage:?} waits for a value its own signal would have to produce"
            );
            assert_eq!(
                stage.prerequisite(),
                previous,
                "{stage:?} skips a stage — the ladder must have no holes"
            );
            previous = stage.value();
        }
    }

    /// Labels are a boot-log contract AND the `@stage` wire token: the round trip must hold,
    /// and an unknown token must never resolve to a stage.
    #[test]
    fn test_reject_unknown_stage_label() {
        for stage in Stage::ALL {
            assert_eq!(Stage::from_label(stage.label().as_bytes()), Some(stage));
        }
        for bogus in [&b"shell_visible"[..], b"", b"display ready", b"SHELL-VISIBLE", b"5"] {
            assert_eq!(Stage::from_label(bogus), None, "bogus label resolved to a stage");
        }
    }

    /// The verb must fit the routing frame it travels in (`@stage <label>`, one
    /// `OP_ROUTE_GET` name field of at most 48 bytes).
    #[test]
    fn stage_verb_fits_the_routing_frame() {
        for stage in Stage::ALL {
            assert!("@stage ".len() + stage.label().len() <= 48);
        }
    }
}
