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
    const MSG_OFFSET: i32 = 0x44;
    const LABEL_PUTS: i32 = 0x1c;
    const LABEL_DONE: i32 = 0x40;
    const LABEL_WAIT_READY: i32 = 0x28;

    let mut text = Vec::new();
    let mut pc = 0i32;

    push(&mut text, encode_auipc(10, 0));
    pc += 4;

    push(&mut text, encode_addi(10, 10, MSG_OFFSET));
    pc += 4;

    push(&mut text, encode_jal(1, LABEL_PUTS - pc));
    pc += 4;

    push(&mut text, encode_addi(10, 0, 0));
    pc += 4;

    push(&mut text, encode_addi(17, 0, 11));
    pc += 4;

    push(&mut text, 0x00000073); // ecall
    pc += 4;

    push(&mut text, 0x0000006f); // jal x0, 0
    pc += 4;

    push(&mut text, encode_lbu(5, 10, 0));
    pc += 4;

    push(&mut text, encode_beq(5, 0, LABEL_DONE - pc));
    pc += 4;

    push(&mut text, encode_lui(6, 0x10000));
    pc += 4;

    push(&mut text, encode_lbu(7, 6, 5));
    pc += 4;

    push(&mut text, encode_andi(7, 7, 0x20));
    pc += 4;

    push(&mut text, encode_beq(7, 0, LABEL_WAIT_READY - pc));
    pc += 4;

    push(&mut text, encode_sb(5, 6, 0));
    pc += 4;

    push(&mut text, encode_addi(10, 10, 1));
    pc += 4;

    push(&mut text, encode_jal(0, LABEL_PUTS - pc));
    pc += 4;

    push(&mut text, 0x00008067); // jalr x0, 0(x1)
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
