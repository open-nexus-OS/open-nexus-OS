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

/// FNV-1a over a byte slice — the digest both the host golden test and the
/// QEMU probe use to compare raster output (shared SSOT, cannot drift).
#[must_use]
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// The D4 proof workload: source, target size and the HOST-PINNED digest of
/// its 32×32 BGRA raster (`proof_svg_digest_matches_pinned` regenerates it —
/// if nexus-svg's output changes legitimately, that test fails first and the
/// constant is updated in the same commit).
pub const PROOF_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32">
  <rect x="2" y="2" width="28" height="28" fill="#284a6e"/>
  <circle cx="12" cy="14" r="8" fill="#e0a33c" fill-opacity="0.8"/>
  <circle cx="20" cy="18" r="9" fill="#5ac8fa" fill-opacity="0.6"/>
</svg>"##;
pub const PROOF_SVG_W: usize = 32;
pub const PROOF_SVG_H: usize = 32;
pub const PROOF_SVG_DIGEST: u64 = 0x0c90_82d6_1484_0ed1;

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
    fn proof_svg_digest_matches_pinned() {
        let doc = nexus_svg::parse_svg(super::PROOF_SVG).expect("parse");
        let out = nexus_svg::rasterize_document_at(
            &doc,
            super::PROOF_SVG_W as u32,
            super::PROOF_SVG_H as u32,
        )
        .expect("raster");
        let digest = super::fnv1a(&out.buffer);
        assert_eq!(
            digest,
            super::PROOF_SVG_DIGEST,
            "raster digest changed: update PROOF_SVG_DIGEST to {digest:#018x}"
        );
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
