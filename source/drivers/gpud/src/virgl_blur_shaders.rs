// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TGSI source for the separable gaussian backdrop blur — the two
//! fragment shaders `virgl_blur_init` creates and `blur_rt_backdrop` binds.
//! Shader text is DATA, so it lives apart from the bring-up logic that submits
//! it (structure-gate: `backend/virgl3d.rs` is at its LOC ratchet).
//! OWNERS: @ui @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: proven at boot — `gpud: rt backdrop dst-so-far` + the visible
//! glass frame (a malformed TGSI program fails the CREATE_OBJECT silently, so
//! the marker plus a non-flat glass surface is the honest signal).

#![cfg(all(feature = "virgl", feature = "os-lite", target_os = "none"))]

/// Separable gaussian fragment shader. CONST[0] = (inv_w, inv_h, radius,
/// k = -1/(2σ²·ln2)); CONST[1] = (dir_x, dir_y, origin_x, origin_y).
/// Per tap: weight = 2^(k·i²) (≡ exp(-i²/2σ²)); the weight sum normalizes,
/// matching the CPU reference in `blur_backdrop_separable_vmo`.
pub(crate) const FS_BLUR: &str = "FRAG\n\
    DCL IN[0], POSITION, LINEAR\n\
    DCL OUT[0], COLOR\n\
    DCL SAMP[0]\n\
    DCL SVIEW[0], 2D, FLOAT\n\
    DCL CONST[0..1]\n\
    DCL TEMP[0..5]\n\
    DCL ADDR[0]\n\
    IMM[0] FLT32 { 0.0000, 1.0000, -1.0000, 0.5000}\n\
    MOV TEMP[0], IMM[0].xxxx\n\
    MOV TEMP[1].x, IMM[0].xxxx\n\
    MUL TEMP[2].x, CONST[0].zzzz, IMM[0].zzzz\n\
    BGNLOOP\n\
    SGT TEMP[3].x, TEMP[2].xxxx, CONST[0].zzzz\n\
    IF TEMP[3].xxxx\n\
    BRK\n\
    ENDIF\n\
    MUL TEMP[3].x, TEMP[2].xxxx, TEMP[2].xxxx\n\
    MUL TEMP[3].x, TEMP[3].xxxx, CONST[0].wwww\n\
    EX2 TEMP[3].x, TEMP[3].xxxx\n\
    MAD TEMP[4].xy, CONST[1].xyyy, TEMP[2].xxxx, IN[0].xyyy\n\
    ADD TEMP[4].xy, TEMP[4].xyyy, CONST[1].zwww\n\
    MUL TEMP[4].xy, TEMP[4].xyyy, CONST[0].xyyy\n\
    TEX TEMP[5], TEMP[4], SAMP[0], 2D\n\
    MAD TEMP[0], TEMP[5], TEMP[3].xxxx, TEMP[0]\n\
    ADD TEMP[1].x, TEMP[1].xxxx, TEMP[3].xxxx\n\
    ADD TEMP[2].x, TEMP[2].xxxx, IMM[0].yyyy\n\
    ENDLOOP\n\
    RCP TEMP[1].x, TEMP[1].xxxx\n\
    MUL OUT[0], TEMP[0], TEMP[1].xxxx\n\
    END\n";

/// [`FS_BLUR`] with the layer's ROUNDED-RECT coverage on the output alpha — the
/// FINAL (vertical) pass of a glass backdrop blur. Without it the blur lands as
/// a hard RECTANGLE while the glass fill on top is rounded, so every pill and
/// circle showed blurred backdrop standing outside its own corners. The SDF is
/// the same analytic one `FS_LAYER` uses for the content, so the blur edge and
/// the fill edge are one curve.
///
/// CONST[0..1] as in [`FS_BLUR`]; CONST[2] = (-cx, -cy, bx, by) — rect centre
/// (negated) + half-extents minus radius; CONST[3] = (radius, …). Draw it with
/// an alpha-"over" blend: outside the shape alpha is 0, so the render target
/// keeps whatever was already composited there.
pub(crate) const FS_BLUR_ROUND: &str = "FRAG\n\
    DCL IN[0], POSITION, LINEAR\n\
    DCL OUT[0], COLOR\n\
    DCL SAMP[0]\n\
    DCL SVIEW[0], 2D, FLOAT\n\
    DCL CONST[0..3]\n\
    DCL TEMP[0..8]\n\
    DCL ADDR[0]\n\
    IMM[0] FLT32 { 0.0000, 1.0000, -1.0000, 0.5000}\n\
    MOV TEMP[0], IMM[0].xxxx\n\
    MOV TEMP[1].x, IMM[0].xxxx\n\
    MUL TEMP[2].x, CONST[0].zzzz, IMM[0].zzzz\n\
    BGNLOOP\n\
    SGT TEMP[3].x, TEMP[2].xxxx, CONST[0].zzzz\n\
    IF TEMP[3].xxxx\n\
    BRK\n\
    ENDIF\n\
    MUL TEMP[3].x, TEMP[2].xxxx, TEMP[2].xxxx\n\
    MUL TEMP[3].x, TEMP[3].xxxx, CONST[0].wwww\n\
    EX2 TEMP[3].x, TEMP[3].xxxx\n\
    MAD TEMP[4].xy, CONST[1].xyyy, TEMP[2].xxxx, IN[0].xyyy\n\
    ADD TEMP[4].xy, TEMP[4].xyyy, CONST[1].zwww\n\
    MUL TEMP[4].xy, TEMP[4].xyyy, CONST[0].xyyy\n\
    TEX TEMP[5], TEMP[4], SAMP[0], 2D\n\
    MAD TEMP[0], TEMP[5], TEMP[3].xxxx, TEMP[0]\n\
    ADD TEMP[1].x, TEMP[1].xxxx, TEMP[3].xxxx\n\
    ADD TEMP[2].x, TEMP[2].xxxx, IMM[0].yyyy\n\
    ENDLOOP\n\
    RCP TEMP[1].x, TEMP[1].xxxx\n\
    MUL TEMP[0], TEMP[0], TEMP[1].xxxx\n\
    ADD TEMP[6].xy, IN[0].xyyy, CONST[2].xyyy\n\
    MAX TEMP[6].xy, TEMP[6].xyyy, -TEMP[6].xyyy\n\
    ADD TEMP[6].xy, TEMP[6].xyyy, -CONST[2].zwww\n\
    MAX TEMP[7].xy, TEMP[6].xyyy, IMM[0].xxxx\n\
    DP2 TEMP[8].x, TEMP[7].xyyy, TEMP[7].xyyy\n\
    SQRT TEMP[8].x, TEMP[8].xxxx\n\
    MAX TEMP[7].x, TEMP[6].xxxx, TEMP[6].yyyy\n\
    MIN TEMP[7].x, TEMP[7].xxxx, IMM[0].xxxx\n\
    ADD TEMP[8].x, TEMP[8].xxxx, TEMP[7].xxxx\n\
    ADD TEMP[8].x, TEMP[8].xxxx, -CONST[3].xxxx\n\
    ADD TEMP[7].x, IMM[0].wwww, -TEMP[8].xxxx\n\
    MAX TEMP[7].x, TEMP[7].xxxx, IMM[0].xxxx\n\
    MIN TEMP[7].x, TEMP[7].xxxx, IMM[0].yyyy\n\
    MUL TEMP[0].w, TEMP[0].wwww, TEMP[7].xxxx\n\
    MOV OUT[0], TEMP[0]\n\
    END\n";
