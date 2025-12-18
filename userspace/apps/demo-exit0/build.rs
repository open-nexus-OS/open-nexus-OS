// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Build script for demo-exit0 application ELF generation
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! ADR: docs/adr/0007-executable-payloads-architecture.md

use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");

    let text = build_text();
    let elf_bytes = build_elf(&text);

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let out_path = out_dir.join("demo-exit0.elf");
    fs::write(&out_path, elf_bytes).expect("write demo-exit0");
}

fn build_elf(text: &[u8]) -> Vec<u8> {
    const ENTRY: u64 = 0x1000;
    const TEXT_OFFSET: usize = 0x100;

    let mut header = Vec::with_capacity(64);
    header.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    header.extend_from_slice(&2u16.to_le_bytes());
    header.extend_from_slice(&243u16.to_le_bytes());
    header.extend_from_slice(&1u32.to_le_bytes());
    header.extend_from_slice(&ENTRY.to_le_bytes());
    header.extend_from_slice(&(64u64).to_le_bytes());
    header.extend_from_slice(&0u64.to_le_bytes());
    header.extend_from_slice(&0u32.to_le_bytes());
    header.extend_from_slice(&64u16.to_le_bytes());
    header.extend_from_slice(&56u16.to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&0u16.to_le_bytes());
    header.extend_from_slice(&0u16.to_le_bytes());
    header.extend_from_slice(&0u16.to_le_bytes());
    assert_eq!(header.len(), 64);

    let filesz = text.len() as u64;
    let mut ph = Vec::with_capacity(56);
    ph.extend_from_slice(&1u32.to_le_bytes());
    ph.extend_from_slice(&(0x5u32).to_le_bytes());
    ph.extend_from_slice(&(TEXT_OFFSET as u64).to_le_bytes());
    ph.extend_from_slice(&ENTRY.to_le_bytes());
    ph.extend_from_slice(&ENTRY.to_le_bytes());
    ph.extend_from_slice(&filesz.to_le_bytes());
    ph.extend_from_slice(&filesz.to_le_bytes());
    ph.extend_from_slice(&0x1000u64.to_le_bytes());
    assert_eq!(ph.len(), 56);

    let mut elf = Vec::new();
    elf.extend_from_slice(&header);
    elf.extend_from_slice(&ph);
    if elf.len() < TEXT_OFFSET {
        elf.resize(TEXT_OFFSET, 0);
    }
    elf.extend_from_slice(text);
    elf
}

fn build_text() -> Vec<u8> {
    // This payload runs in U-mode, so it must NOT touch the UART MMIO directly.
    // Instead, print via the kernel debug_putc syscall and then exit(0).
    //
    // Syscalls:
    //   - a7=16: debug_putc(a0=byte)
    //   - a7=11: exit(a0=status)
    const SYSCALL_DEBUG_PUTC: i32 = 16;
    const SYSCALL_EXIT: i32 = 11;

    // Layout: code first, then message bytes. We use AUIPC to get PC-relative address.
    // Code size is 12 instructions = 48 bytes, so the message starts at 0x30.
    const MSG_OFFSET: i32 = 0x30;
    const LABEL_LOOP: i32 = 0x08;
    const LABEL_DONE: i32 = 0x20;

    let mut text = Vec::new();
    let mut pc = 0i32;

    // s0 (x8) holds the message pointer across syscalls.
    push(&mut text, encode_auipc(8, 0));
    pc += 4;

    push(&mut text, encode_addi(8, 8, MSG_OFFSET));
    pc += 4;

    // loop:
    //   a0 = *(u8*)s0
    //   if a0==0 goto done
    //   a7 = debug_putc; ecall
    //   s0++; goto loop
    push(&mut text, encode_lbu(10, 8, 0));
    pc += 4;

    push(&mut text, encode_beq(10, 0, LABEL_DONE - pc));
    pc += 4;

    push(&mut text, encode_addi(17, 0, SYSCALL_DEBUG_PUTC));
    pc += 4;

    push(&mut text, 0x00000073); // ecall
    pc += 4;

    push(&mut text, encode_addi(8, 8, 1));
    pc += 4;

    push(&mut text, encode_jal(0, LABEL_LOOP - pc));
    pc += 4;

    // done: exit(0)
    push(&mut text, encode_addi(10, 0, 0)); // a0=status
    pc += 4;
    push(&mut text, encode_addi(17, 0, SYSCALL_EXIT));
    pc += 4;
    push(&mut text, 0x00000073); // ecall
    pc += 4;
    push(&mut text, 0x0000006f); // jal x0, 0 (should never return)
    pc += 4;

    assert_eq!(pc, MSG_OFFSET, "message offset mismatch");

    text.extend_from_slice(b"child: exit0 start\n\0");
    text
}

fn push(out: &mut Vec<u8>, instr: u32) {
    out.extend_from_slice(&instr.to_le_bytes());
}

fn encode_auipc(rd: u8, imm20: i32) -> u32 {
    ((imm20 as u32) << 12) | ((rd as u32) << 7) | 0x17
}

fn encode_addi(rd: u8, rs1: u8, imm: i32) -> u32 {
    encode_i_type(0x13, rd, rs1, imm, 0)
}

fn encode_andi(rd: u8, rs1: u8, imm: i32) -> u32 {
    encode_i_type(0x13, rd, rs1, imm, 0b111)
}

fn encode_lbu(rd: u8, rs1: u8, imm: i32) -> u32 {
    encode_i_type(0x03, rd, rs1, imm, 0b100)
}

fn encode_i_type(opcode: u32, rd: u8, rs1: u8, imm: i32, funct3: u32) -> u32 {
    let imm_masked = (imm as u32) & 0xfff;
    (imm_masked << 20) | ((rs1 as u32) << 15) | (funct3 << 12) | ((rd as u32) << 7) | opcode
}

fn encode_lui(rd: u8, imm20: i32) -> u32 {
    ((imm20 as u32) << 12) | ((rd as u32) << 7) | 0x37
}

fn encode_sb(rs2: u8, rs1: u8, imm: i32) -> u32 {
    let imm_u = imm as u32;
    (((imm_u >> 5) & 0x7f) << 25)
        | ((rs2 as u32) << 20)
        | ((rs1 as u32) << 15)
        | (0b000 << 12)
        | ((imm_u & 0x1f) << 7)
        | 0x23
}

fn encode_beq(rs1: u8, rs2: u8, offset: i32) -> u32 {
    encode_branch(0b000, rs1, rs2, offset)
}

fn encode_branch(funct3: u32, rs1: u8, rs2: u8, offset: i32) -> u32 {
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

fn encode_jal(rd: u8, offset: i32) -> u32 {
    let imm = offset as u32;
    let bit20 = ((imm >> 20) & 0x1) << 31;
    let bits10_1 = ((imm >> 1) & 0x3ff) << 21;
    let bit11 = ((imm >> 11) & 0x1) << 20;
    let bits19_12 = ((imm >> 12) & 0xff) << 12;
    bit20 | bits19_12 | bit11 | bits10_1 | ((rd as u32) << 7) | 0x6f
}
