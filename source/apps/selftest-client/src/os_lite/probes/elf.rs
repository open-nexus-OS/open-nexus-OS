use exec_payloads::HELLO_ELF;

use crate::markers::{emit_byte, emit_bytes, emit_hex_u64, emit_line};

pub(crate) fn log_hello_elf_header() {
    if HELLO_ELF.len() < 64 {
        emit_line("^hello elf too small");
        return;
    }
    let entry = read_u64_le(HELLO_ELF, 24);
    let phoff = read_u64_le(HELLO_ELF, 32);
    emit_bytes(b"^hello entry=0x");
    emit_hex_u64(entry);
    emit_bytes(b" phoff=0x");
    emit_hex_u64(phoff);
    emit_byte(b'\n');
    if (phoff as usize) + 56 <= HELLO_ELF.len() {
        let p_offset = read_u64_le(HELLO_ELF, phoff as usize + 8);
        let p_vaddr = read_u64_le(HELLO_ELF, phoff as usize + 16);
        emit_bytes(b"^hello p_offset=0x");
        emit_hex_u64(p_offset);
        emit_bytes(b" p_vaddr=0x");
        emit_hex_u64(p_vaddr);
        emit_byte(b'\n');
    }
}

fn read_u64_le(bytes: &[u8], off: usize) -> u64 {
    if off + 8 > bytes.len() {
        return 0;
    }
    u64::from_le_bytes([
        bytes[off],
        bytes[off + 1],
        bytes[off + 2],
        bytes[off + 3],
        bytes[off + 4],
        bytes[off + 5],
        bytes[off + 6],
        bytes[off + 7],
    ])
}
