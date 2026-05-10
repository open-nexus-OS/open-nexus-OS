// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Build script for demo-exit0 application ELF generation
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! ADR: docs/adr/0007-executable-payloads-architecture.md

use std::{cell::Cell, env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/lib.rs");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));

    let exit0 = build_elf(&build_text(0, b"child: exit0 start\n\0"));
    let out_path = out_dir.join("demo-exit0.elf");
    fs::write(&out_path, exit0).expect("write demo-exit0");

    let exit42 = build_elf(&build_text(42, b"child: exit42 start\n\0"));
    let out_path = out_dir.join("demo-exit42.elf");
    fs::write(&out_path, exit42).expect("write demo-exit42");

    let minidump = build_elf(&build_minidump_text());
    let out_path = out_dir.join("demo-minidump.elf");
    fs::write(&out_path, minidump).expect("write demo-minidump");

    let vmo_consumer = build_elf(&build_vmo_consumer_text());
    let out_path = out_dir.join("demo-vmo-consumer.elf");
    fs::write(&out_path, vmo_consumer).expect("write demo-vmo-consumer");
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
    // Kernel exec loader bring-up hardening:
    // pad the ELF blob to a 4-byte boundary so kernel-side parsing that reads u32 chunks
    // never needs to touch an unmapped trailing byte.
    while (elf.len() & 0x3) != 0 {
        elf.push(0);
    }
    elf
}

fn build_text(exit_code: i32, msg: &[u8]) -> Vec<u8> {
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

    // done: exit(exit_code)
    push(&mut text, encode_addi(10, 0, exit_code)); // a0=status
    pc += 4;
    push(&mut text, encode_addi(17, 0, SYSCALL_EXIT));
    pc += 4;
    push(&mut text, 0x00000073); // ecall
    pc += 4;
    push(&mut text, 0x0000006f); // jal x0, 0 (should never return)
    pc += 4;

    assert_eq!(pc, MSG_OFFSET, "message offset mismatch");

    text.extend_from_slice(msg);
    text
}

fn build_minidump_text() -> Vec<u8> {
    const SYSCALL_YIELD: i32 = 0;
    const SYSCALL_EXIT: i32 = 11;
    const SYSCALL_IPC_SEND_V1: i32 = 14;
    const SYSCALL_DEBUG_PUTC: i32 = 16;
    const SYSCALL_IPC_RECV_V1: i32 = 18;
    const STACK_SCRATCH_SIZE: i32 = 64;
    const HDR_RSP_OFF: i32 = 0;
    const STATEFS_RSP_OFF: i32 = 16;
    const RSP_MAX: i32 = 32;
    const STATEFS_SEND_SLOT: i32 = 7;
    const STATEFS_RECV_SLOT: i32 = 8;

    const MSG: &[u8] = b"child: minidump start\n\0";
    const DUMP_NAME: &[u8] = b"demo.minidump";
    const DUMP_PATH: &str = "/state/crash/child.demo.minidump.nmd";
    let build_id = deterministic_build_id_bytes(DUMP_NAME);
    let dump_value = build_minidump_value(DUMP_NAME, &build_id);
    let put_req = build_statefs_put_request(DUMP_PATH.as_bytes(), &dump_value);

    let mut data = Vec::new();
    let msg_off = 0usize;
    data.extend_from_slice(MSG);
    while (data.len() & 0x3) != 0 {
        data.push(0);
    }
    let put_hdr_off = data.len();
    data.extend_from_slice(&build_msg_header(put_req.len() as u32));
    let put_req_off = data.len();
    data.extend_from_slice(&put_req);

    let mut ins = Vec::<u32>::new();
    let ins_count = Cell::new(0usize);
    let mut emit = |op: u32| {
        ins.push(op);
        ins_count.set(ins_count.get() + 1);
    };

    emit(encode_auipc(8, 0)); // s0 = code pc
    let patch_data_base_idx = ins_count.get();
    emit(0); // addi s0, s0, data_off
    let patch_msg_ptr_idx = ins_count.get();
    emit(0); // addi s4, s0, msg_off

    let loop_idx = ins_count.get();
    emit(encode_lbu(10, 20, 0)); // a0 = *s4
    let beq_done_print_idx = ins_count.get();
    emit(0); // beq a0, x0, done_print
    emit(encode_addi(17, 0, SYSCALL_DEBUG_PUTC));
    emit(0x00000073); // ecall
    emit(encode_addi(20, 20, 1)); // s4++
    let jal_loop_idx = ins_count.get();
    emit(0); // jal x0, loop
    let done_print_idx = ins_count.get();

    emit(encode_addi(2, 2, -STACK_SCRATCH_SIZE)); // sp -= scratch

    // Give execd a bounded window to transfer statefs slots into 7/8.
    emit(encode_addi(19, 0, 64)); // s3 retries
    let yield_loop_idx = ins_count.get();
    emit(encode_addi(17, 0, SYSCALL_YIELD));
    emit(0x00000073); // ecall
    emit(encode_addi(19, 19, -1));
    let beq_yield_done_idx = ins_count.get();
    emit(0); // beq s3, x0, after_yield
    let jal_yield_loop_idx = ins_count.get();
    emit(0); // jal x0, yield_loop
    let after_yield_idx = ins_count.get();

    // statefs PUT via transferred slots 7/8
    emit(encode_addi(10, 0, STATEFS_SEND_SLOT));
    let put_send_hdr_ptr_idx = ins_count.get();
    emit(0); // a1 = &put hdr
    let put_send_req_ptr_idx = ins_count.get();
    emit(0); // a2 = &put req
    emit(encode_addi(13, 0, put_req.len() as i32));
    emit(encode_addi(14, 0, 0));
    emit(encode_addi(15, 0, 0));
    emit(encode_addi(17, 0, SYSCALL_IPC_SEND_V1));
    emit(0x00000073); // ecall
    let blt_put_send_fail_idx = ins_count.get();
    emit(0); // blt a0, x0, fail

    emit(encode_addi(10, 0, STATEFS_RECV_SLOT));
    emit(encode_addi(11, 2, HDR_RSP_OFF));
    emit(encode_addi(12, 2, STATEFS_RSP_OFF));
    emit(encode_addi(13, 0, RSP_MAX));
    emit(encode_addi(14, 0, 0));
    emit(encode_addi(15, 0, 0));
    emit(encode_addi(17, 0, SYSCALL_IPC_RECV_V1));
    emit(0x00000073); // ecall
    let blt_put_recv_fail_idx = ins_count.get();
    emit(0); // blt a0, x0, fail
    emit(encode_lbu(6, 2, STATEFS_RSP_OFF));
    emit(encode_addi(7, 0, b'S' as i32));
    let bne_put_magic0_fail_idx = ins_count.get();
    emit(0);
    emit(encode_lbu(6, 2, STATEFS_RSP_OFF + 1));
    emit(encode_addi(7, 0, b'F' as i32));
    let bne_put_magic1_fail_idx = ins_count.get();
    emit(0);
    emit(encode_lbu(6, 2, STATEFS_RSP_OFF + 2));
    emit(encode_addi(7, 0, 1));
    let bne_put_version_fail_idx = ins_count.get();
    emit(0);
    emit(encode_lbu(6, 2, STATEFS_RSP_OFF + 3));
    emit(encode_addi(7, 0, 0x81));
    let bne_put_op_fail_idx = ins_count.get();
    emit(0);
    emit(encode_lbu(6, 2, STATEFS_RSP_OFF + 4));
    emit(encode_addi(7, 0, 0));
    let bne_put_status_fail_idx = ins_count.get();
    emit(0);

    emit(encode_addi(10, 0, 42)); // success exit
    emit(encode_addi(17, 0, SYSCALL_EXIT));
    emit(0x00000073); // ecall
    emit(0x0000006f); // jal x0, 0

    let fail_put_send_idx = ins_count.get();
    emit(encode_addi(10, 0, 33));
    emit(encode_addi(17, 0, SYSCALL_EXIT));
    emit(0x00000073); // ecall
    emit(0x0000006f); // jal x0, 0

    let fail_put_recv_idx = ins_count.get();
    emit(encode_addi(10, 0, 34));
    emit(encode_addi(17, 0, SYSCALL_EXIT));
    emit(0x00000073); // ecall
    emit(0x0000006f); // jal x0, 0

    let code_size = ins_count.get() * 4;
    assert!(code_size < 2048, "minidump payload code too large");

    ins[patch_data_base_idx] = encode_addi(8, 8, code_size as i32);
    ins[patch_msg_ptr_idx] = encode_addi(20, 8, msg_off as i32);

    let beq_done_off = ((done_print_idx as i32 - beq_done_print_idx as i32) * 4) as i32;
    ins[beq_done_print_idx] = encode_beq(10, 0, beq_done_off);
    let jal_loop_off = ((loop_idx as i32 - jal_loop_idx as i32) * 4) as i32;
    ins[jal_loop_idx] = encode_jal(0, jal_loop_off);
    let beq_yield_done_off = ((after_yield_idx as i32 - beq_yield_done_idx as i32) * 4) as i32;
    ins[beq_yield_done_idx] = encode_beq(19, 0, beq_yield_done_off);
    let jal_yield_loop_off = ((yield_loop_idx as i32 - jal_yield_loop_idx as i32) * 4) as i32;
    ins[jal_yield_loop_idx] = encode_jal(0, jal_yield_loop_off);

    ins[put_send_hdr_ptr_idx] = encode_addi(11, 8, put_hdr_off as i32);
    ins[put_send_req_ptr_idx] = encode_addi(12, 8, put_req_off as i32);

    let patch_fail_branch = |ins: &mut [u32], idx: usize, target: usize| {
        let off = ((target as i32 - idx as i32) * 4) as i32;
        ins[idx] = encode_blt(10, 0, off);
    };
    patch_fail_branch(&mut ins, blt_put_send_fail_idx, fail_put_send_idx);
    patch_fail_branch(&mut ins, blt_put_recv_fail_idx, fail_put_recv_idx);
    let patch_bne_fail = |ins: &mut [u32], idx: usize, target: usize| {
        let off = ((target as i32 - idx as i32) * 4) as i32;
        ins[idx] = encode_bne(6, 7, off);
    };
    patch_bne_fail(&mut ins, bne_put_magic0_fail_idx, fail_put_recv_idx);
    patch_bne_fail(&mut ins, bne_put_magic1_fail_idx, fail_put_recv_idx);
    patch_bne_fail(&mut ins, bne_put_version_fail_idx, fail_put_recv_idx);
    patch_bne_fail(&mut ins, bne_put_op_fail_idx, fail_put_recv_idx);
    patch_bne_fail(&mut ins, bne_put_status_fail_idx, fail_put_recv_idx);

    let mut text = Vec::with_capacity(code_size + data.len());
    for op in ins {
        text.extend_from_slice(&op.to_le_bytes());
    }
    text.extend_from_slice(&data);
    text
}

fn build_vmo_consumer_text() -> Vec<u8> {
    const SYSCALL_YIELD: i32 = 0;
    const SYSCALL_EXIT: i32 = 11;
    const SYSCALL_MAP: i32 = 4;
    const VMO_SLOT: i32 = 23;
    const MAP_FLAGS: i32 = 0x13; // VALID | READ | USER
    const MAP_VA: u32 = 0x2100_0000;
    const RETRIES: i32 = 128;
    const EXPECTED: &[u8] = b"task-0031-vmo-share-probe";

    let mut data = Vec::new();
    let expected_off = 0usize;
    data.extend_from_slice(EXPECTED);
    while (data.len() & 0x3) != 0 {
        data.push(0);
    }

    let mut ins = Vec::<u32>::new();
    let count = Cell::new(0usize);
    let mut emit = |op: u32| {
        ins.push(op);
        count.set(count.get() + 1);
    };

    // s0 = data base (PC-relative)
    emit(encode_auipc(8, 0));
    let patch_data_base_idx = count.get();
    emit(0); // addi s0, s0, code_size

    emit(encode_addi(9, 0, RETRIES)); // s1 retries

    let retry_idx = count.get();
    emit(encode_addi(10, 0, VMO_SLOT)); // a0 slot(handle)
    let (map_hi, map_lo) = split_hi_lo_u32(MAP_VA);
    emit(encode_lui(11, map_hi)); // a1 = map va hi
    emit(encode_addi(11, 11, map_lo)); // a1 += lo
    emit(encode_addi(12, 0, 0)); // a2 offset
    emit(encode_addi(13, 0, MAP_FLAGS)); // a3 flags
    emit(encode_addi(17, 0, SYSCALL_MAP));
    emit(0x00000073); // ecall
    let blt_map_fail_idx = count.get();
    emit(0); // blt a0, x0, map_fail
    let jal_mapped_idx = count.get();
    emit(0); // jal x0, mapped

    let map_fail_idx = count.get();
    emit(encode_addi(17, 0, SYSCALL_YIELD));
    emit(0x00000073); // ecall
    emit(encode_addi(9, 9, -1)); // retries--
    let beq_fail_idx = count.get();
    emit(0); // beq s1, x0, fail
    let jal_retry_idx = count.get();
    emit(0); // jal x0, retry

    let mapped_idx = count.get();
    // s2 = mapped va
    emit(encode_lui(18, map_hi));
    emit(encode_addi(18, 18, map_lo));
    // s3 = expected ptr
    let patch_expected_ptr_idx = count.get();
    emit(0); // addi s3, s0, expected_off
    emit(encode_addi(20, 0, EXPECTED.len() as i32)); // s4 = remaining

    let cmp_loop_idx = count.get();
    emit(encode_lbu(5, 18, 0)); // t0 = *s2
    emit(encode_lbu(6, 19, 0)); // t1 = *s3
    let bne_cmp_fail_idx = count.get();
    emit(0); // bne t0, t1, fail
    emit(encode_addi(18, 18, 1)); // s2++
    emit(encode_addi(19, 19, 1)); // s3++
    emit(encode_addi(20, 20, -1)); // s4--
    let bne_cmp_loop_idx = count.get();
    emit(0); // bne s4, x0, cmp_loop

    // success: exit(0)
    emit(encode_addi(10, 0, 0));
    emit(encode_addi(17, 0, SYSCALL_EXIT));
    emit(0x00000073);
    emit(0x0000006f);

    let fail_idx = count.get();
    emit(encode_addi(10, 0, 41));
    emit(encode_addi(17, 0, SYSCALL_EXIT));
    emit(0x00000073);
    emit(0x0000006f);

    let code_size = count.get() * 4;
    assert!(code_size < 2048, "vmo consumer payload code too large");

    ins[patch_data_base_idx] = encode_addi(8, 8, code_size as i32);
    ins[patch_expected_ptr_idx] = encode_addi(19, 8, expected_off as i32);

    let patch_blt = |ins: &mut [u32], idx: usize, target: usize| {
        let off = ((target as i32 - idx as i32) * 4) as i32;
        ins[idx] = encode_blt(10, 0, off);
    };
    let patch_beq = |ins: &mut [u32], rs1: u8, rs2: u8, idx: usize, target: usize| {
        let off = ((target as i32 - idx as i32) * 4) as i32;
        ins[idx] = encode_beq(rs1, rs2, off);
    };
    let patch_bne = |ins: &mut [u32], rs1: u8, rs2: u8, idx: usize, target: usize| {
        let off = ((target as i32 - idx as i32) * 4) as i32;
        ins[idx] = encode_bne(rs1, rs2, off);
    };
    let patch_jal = |ins: &mut [u32], idx: usize, target: usize| {
        let off = ((target as i32 - idx as i32) * 4) as i32;
        ins[idx] = encode_jal(0, off);
    };

    patch_blt(&mut ins, blt_map_fail_idx, map_fail_idx);
    patch_jal(&mut ins, jal_mapped_idx, mapped_idx);
    patch_beq(&mut ins, 9, 0, beq_fail_idx, fail_idx);
    patch_jal(&mut ins, jal_retry_idx, retry_idx);
    patch_bne(&mut ins, 5, 6, bne_cmp_fail_idx, fail_idx);
    patch_bne(&mut ins, 20, 0, bne_cmp_loop_idx, cmp_loop_idx);

    let mut text = Vec::with_capacity(code_size + data.len());
    for op in ins {
        text.extend_from_slice(&op.to_le_bytes());
    }
    text.extend_from_slice(&data);
    text
}

fn push(out: &mut Vec<u8>, instr: u32) {
    out.extend_from_slice(&instr.to_le_bytes());
}

fn encode_auipc(rd: u8, imm20: i32) -> u32 {
    ((imm20 as u32) << 12) | ((rd as u32) << 7) | 0x17
}

fn encode_lui(rd: u8, imm20: i32) -> u32 {
    ((imm20 as u32) << 12) | ((rd as u32) << 7) | 0x37
}

fn encode_addi(rd: u8, rs1: u8, imm: i32) -> u32 {
    encode_i_type(0x13, rd, rs1, imm, 0)
}

fn encode_lbu(rd: u8, rs1: u8, imm: i32) -> u32 {
    encode_i_type(0x03, rd, rs1, imm, 0b100)
}

fn encode_i_type(opcode: u32, rd: u8, rs1: u8, imm: i32, funct3: u32) -> u32 {
    let imm_masked = (imm as u32) & 0xfff;
    (imm_masked << 20) | ((rs1 as u32) << 15) | (funct3 << 12) | ((rd as u32) << 7) | opcode
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

fn encode_blt(rs1: u8, rs2: u8, offset: i32) -> u32 {
    encode_branch(0b100, rs1, rs2, offset)
}

fn encode_bne(rs1: u8, rs2: u8, offset: i32) -> u32 {
    encode_branch(0b001, rs1, rs2, offset)
}

fn split_hi_lo_u32(value: u32) -> (i32, i32) {
    let signed = value as i64;
    let hi = ((signed + 0x800) >> 12) as i32;
    let lo = (signed - ((hi as i64) << 12)) as i32;
    (hi, lo)
}

fn build_msg_header(len: u32) -> [u8; 16] {
    let mut out = [0u8; 16];
    out[12..16].copy_from_slice(&len.to_le_bytes());
    out
}

fn deterministic_build_id_bytes(name: &[u8]) -> Vec<u8> {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in name {
        h ^= b as u64;
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    let mut out = Vec::with_capacity(17);
    out.push(b'b');
    for shift in (0..16).rev() {
        let nibble = ((h >> (shift * 4)) & 0xf) as u8;
        out.push(if nibble < 10 { b'0' + nibble } else { b'a' + (nibble - 10) });
    }
    out
}

fn build_minidump_value(name: &[u8], build_id: &[u8]) -> Vec<u8> {
    let header_len = 32usize;
    let total_len = header_len + build_id.len() + name.len();
    let mut out = Vec::with_capacity(total_len);
    out.extend_from_slice(b"NMD1");
    out.push(1); // version
    out.push(build_id.len() as u8);
    out.push(name.len() as u8);
    out.push(0); // pc_count
    out.extend_from_slice(&0u16.to_le_bytes()); // stack_len
    out.extend_from_slice(&0u16.to_le_bytes()); // code_len
    out.extend_from_slice(&0u32.to_le_bytes()); // pid (v1 demo payload keeps fixed pid=0)
    out.extend_from_slice(&42i32.to_le_bytes()); // exit code
    out.extend_from_slice(&0u64.to_le_bytes()); // timestamp
    out.extend_from_slice(&0u16.to_le_bytes()); // reserved
    out.extend_from_slice(&(total_len as u16).to_le_bytes());
    out.extend_from_slice(build_id);
    out.extend_from_slice(name);
    out
}

fn build_statefs_put_request(key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(10 + key.len() + value.len());
    out.push(b'S');
    out.push(b'F');
    out.push(1); // protocol version
    out.push(1); // OP_PUT
    out.extend_from_slice(&(key.len() as u16).to_le_bytes());
    out.extend_from_slice(&(value.len() as u32).to_le_bytes());
    out.extend_from_slice(key);
    out.extend_from_slice(value);
    out
}
