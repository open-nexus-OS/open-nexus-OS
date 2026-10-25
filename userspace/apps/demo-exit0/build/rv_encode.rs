// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the RV64 instruction encoders the demo payloads are assembled with — split out of
//! `build.rs` (module-size ratchet, TASK-0333 6c-1a). Build-script only.

pub(crate) fn push(out: &mut Vec<u8>, instr: u32) {
    out.extend_from_slice(&instr.to_le_bytes());
}

pub(crate) fn encode_auipc(rd: u8, imm20: i32) -> u32 {
    ((imm20 as u32) << 12) | ((rd as u32) << 7) | 0x17
}

pub(crate) fn encode_addi(rd: u8, rs1: u8, imm: i32) -> u32 {
    encode_i_type(0x13, rd, rs1, imm, 0)
}

/// `lui rd, imm20` (U-type, opcode 0x37): the upper twenty bits of a constant.
pub(crate) fn encode_lui(rd: u8, imm20: i32) -> u32 {
    (((imm20 as u32) & 0xf_ffff) << 12) | ((rd as u32) << 7) | 0x37
}

/// `sb rs2, imm(rs1)` (S-type, opcode 0x23, funct3 0): one byte stored.
pub(crate) fn encode_sb(rs2: u8, rs1: u8, imm: i32) -> u32 {
    let imm = imm as u32;
    (((imm >> 5) & 0x7f) << 25)
        | ((rs2 as u32) << 20)
        | ((rs1 as u32) << 15)
        | ((imm & 0x1f) << 7)
        | 0x23
}

pub(crate) fn encode_lbu(rd: u8, rs1: u8, imm: i32) -> u32 {
    encode_i_type(0x03, rd, rs1, imm, 0b100)
}

pub(crate) fn encode_i_type(opcode: u32, rd: u8, rs1: u8, imm: i32, funct3: u32) -> u32 {
    let imm_masked = (imm as u32) & 0xfff;
    (imm_masked << 20) | ((rs1 as u32) << 15) | (funct3 << 12) | ((rd as u32) << 7) | opcode
}

pub(crate) fn encode_beq(rs1: u8, rs2: u8, offset: i32) -> u32 {
    encode_branch(0b000, rs1, rs2, offset)
}

pub(crate) fn encode_branch(funct3: u32, rs1: u8, rs2: u8, offset: i32) -> u32 {
    let imm = offset as u32;
    let bit12 = ((imm >> 12) & 0x1) << 31;
    let bit11 = ((imm >> 11) & 0x1) << 7;
    let bits10_5 = ((imm >> 5) & 0x3f) << 25;
    let bits4_1 = ((imm >> 1) & 0xf) << 8;
    bit12
        | bits10_5
        | ((rs2 as u32) << 20)
        | ((rs1 as u32) << 15)
        | (funct3 << 12)
        | bits4_1
        | bit11
        | 0x63
}

pub(crate) fn encode_jal(rd: u8, offset: i32) -> u32 {
    let imm = offset as u32;
    let bit20 = ((imm >> 20) & 0x1) << 31;
    let bits10_1 = ((imm >> 1) & 0x3ff) << 21;
    let bit11 = ((imm >> 11) & 0x1) << 20;
    let bits19_12 = ((imm >> 12) & 0xff) << 12;
    bit20 | bits19_12 | bit11 | bits10_1 | ((rd as u32) << 7) | 0x6f
}

pub(crate) fn encode_blt(rs1: u8, rs2: u8, offset: i32) -> u32 {
    encode_branch(0b100, rs1, rs2, offset)
}

pub(crate) fn encode_bne(rs1: u8, rs2: u8, offset: i32) -> u32 {
    encode_branch(0b001, rs1, rs2, offset)
}

/// `cbo.clean 0(rs1)`: MISC-MEM, funct3 010, rd 0, imm 1 (Zicbom).
pub(crate) fn encode_cbo_clean(rs1: u8) -> u32 {
    encode_i_type(0x0f, 0, rs1, 1, 0b010)
}
