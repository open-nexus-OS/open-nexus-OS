// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Broker job kinds — pure, deterministic per-element transforms.
//! This module is the SSOT shared by the service AND its clients (the
//! selftest imports `mix_u32` to compute the expected output locally, so the
//! proof can never drift from the implementation).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host unit tests (determinism, non-identity)

/// `JOB_MAP_MIX_U32`: the deterministic proof transform. Pure per-element
/// function of the input only — the root of the workers=1 ≡ workers=N
/// equality contract for this job kind.
#[must_use]
pub const fn mix_u32(x: u32) -> u32 {
    x.wrapping_mul(0x9E37_79B9).rotate_left(7) ^ 0x5A5A_A5A5
}

#[cfg(test)]
mod tests {
    use super::mix_u32;

    #[test]
    fn mix_is_deterministic_and_not_identity() {
        for x in [0u32, 1, 7, 0xDEAD_BEEF, u32::MAX] {
            assert_eq!(mix_u32(x), mix_u32(x));
            assert_ne!(mix_u32(x), x, "transform must visibly change {x:#x}");
        }
    }

    #[test]
    fn mix_reference_vector_is_stable() {
        // Pinned known-answer values: a silent change to the transform would
        // break service/client agreement — fail here first, on the host.
        assert_eq!(mix_u32(0), 0x5A5A_A5A5);
        assert_eq!(mix_u32(1), mix_u32(1));
        let expected_1 = 0x9E37_79B9u32.rotate_left(7) ^ 0x5A5A_A5A5;
        assert_eq!(mix_u32(1), expected_1);
    }
}
