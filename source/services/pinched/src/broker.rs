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
//! ADR: docs/adr/0045-pinched-compute-broker-and-backends.md

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

/// Task #14 ingredient probe: a soft-float compute loop with a HOST-PINNED
/// checksum. Shared SSOT — the QEMU selftest runs THIS function and compares
/// against [`F32_SOAK_CHECK`]; a mismatch means the process computes
/// soft-float differently from the host build of the same code.
#[must_use]
pub fn f32_soak_check() -> u32 {
    let mut acc: f32 = 1.0;
    let mut check: u32 = 0;
    for i in 0..50_000u32 {
        acc = acc * 1.000_1 + (i as f32) * 0.000_5;
        if acc > 1_000.0 {
            acc /= 3.0;
        }
        check = check.wrapping_add(acc.to_bits());
    }
    check
}

/// Host-pinned expected value of [`f32_soak_check`] (test regenerates it).
pub const F32_SOAK_CHECK: u32 = 0x3469_f811;

/// Task #14 ingredient probe 2: the libm transcendentals the SVG pipeline
/// leans on (circle flattening = sin/cos, raster rows = floor/ceil, arcs =
/// sqrt). Host-pinned; libm is pure-integer soft-float and must be
/// bit-identical everywhere.
#[must_use]
pub fn libm_soak_check() -> u32 {
    let mut check: u32 = 0;
    for i in 0..1_000u32 {
        let x = (i as f32) * 0.037 - 18.0;
        check = check.wrapping_add(libm::sinf(x).to_bits());
        check = check.wrapping_add(libm::cosf(x).to_bits());
        check = check.wrapping_add(libm::sqrtf(x.max(0.0)).to_bits());
        check = check.wrapping_add(libm::floorf(x * 7.3).to_bits());
        check = check.wrapping_add(libm::ceilf(x * 7.3).to_bits());
        check = check.wrapping_add(libm::roundf(x * 11.9).to_bits());
    }
    check
}

/// Host-pinned expected value of [`libm_soak_check`].
pub const LIBM_SOAK_CHECK: u32 = 0x5ca8_ef9f;

/// Task #14 ingredient probe 3: core's decimal→f32 parsing (dec2flt), which
/// every SVG attribute goes through and no other soak covers.
#[must_use]
pub fn strparse_soak_check() -> u32 {
    let inputs = [
        "0.8",
        "0.6",
        "3.14159",
        "28",
        "2",
        "-18.25",
        "1e3",
        "0.0001",
        "12.5",
        "255",
        "1.000001",
        "9.75",
        "0.30000000000000004",
        "6.02e2",
        "-0.5",
    ];
    let mut check: u32 = 0;
    for s in inputs {
        if let Ok(v) = s.parse::<f32>() {
            check = check.wrapping_add(v.to_bits());
        }
        check = check.wrapping_mul(0x0100_0193);
    }
    check
}

/// Host-pinned expected value of [`strparse_soak_check`].
pub const STRPARSE_SOAK_CHECK: u32 = 0xcba4_0e86;

/// Host-pinned `fnv1a(PROOF_SVG.as_bytes())` — a pure-integer discriminator
/// for the task #14 probes (if THIS diverges in-process, 64-bit integer
/// arithmetic itself is corrupted at that call site).
pub const PROOF_SVG_SRC_FNV: u64 = 0x0d59_8b4c_74a7_2a02;

/// Host-pinned digest of the PROOF_SVG raster PLAN (parse + tessellate,
/// before any rasterization) — splits the pipeline for the task #14 probes.
pub const PROOF_SVG_PLAN_DIGEST: u64 = 0x250e_ad56_fbdc_d1d4;

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
    fn f32_soak_check_matches_pinned() {
        let got = super::f32_soak_check();
        assert_eq!(
            got,
            super::F32_SOAK_CHECK,
            "f32 soak check changed: update F32_SOAK_CHECK to {got:#010x}"
        );
    }

    #[test]
    fn libm_soak_check_matches_pinned() {
        let got = super::libm_soak_check();
        assert_eq!(
            got,
            super::LIBM_SOAK_CHECK,
            "libm soak check changed: update LIBM_SOAK_CHECK to {got:#010x}"
        );
    }

    #[test]
    fn strparse_soak_check_matches_pinned() {
        let got = super::strparse_soak_check();
        assert_eq!(
            got,
            super::STRPARSE_SOAK_CHECK,
            "strparse soak changed: update STRPARSE_SOAK_CHECK to {got:#010x}"
        );
    }

    #[test]
    fn proof_svg_plan_digest_matches_pinned() {
        let doc = nexus_svg::parse_svg(super::PROOF_SVG).expect("parse");
        let plan =
            nexus_svg::plan_document_at(&doc, super::PROOF_SVG_W as u32, super::PROOF_SVG_H as u32)
                .expect("plan");
        let got = plan.debug_digest();
        assert_eq!(
            got,
            super::PROOF_SVG_PLAN_DIGEST,
            "plan digest changed: update PROOF_SVG_PLAN_DIGEST to {got:#018x}"
        );
    }

    #[test]
    fn proof_svg_src_fnv_matches_pinned() {
        let got = super::fnv1a(super::PROOF_SVG.as_bytes());
        assert_eq!(
            got,
            super::PROOF_SVG_SRC_FNV,
            "src fnv changed: update PROOF_SVG_SRC_FNV to {got:#018x}"
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
