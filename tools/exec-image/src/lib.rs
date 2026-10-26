// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: an executable image's layout rule (ADR-0080) and the tiny-ELF builder that
//! satisfies it. The kernel maps a process's read-only and executable segments from its sealed
//! image's own frames and copies its data pages; it refuses an image whose `PT_LOAD`s are off
//! the page grid or share a file page. `layout::check` is that rule on the host — the same
//! decisions, in the same order — so a misbuilt image fails the build that would embed or
//! pack it, not the board. `tiny` builds the probe ELFs (demo-exit0, hello, recv-wake) with
//! their text on the page grid; `rv` holds the RV64 encoders they are assembled with.
//! OWNERS: @runtime @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal (host tooling)
//! TEST_COVERAGE: `tests` below (the rule on built images, each refusal)
//! ADR: docs/adr/0080-a-process-runs-from-a-sealed-image-text-mapped-from-its-frames-data-copied-no-other-exec-path.md

#![forbid(unsafe_code)]

/// The layout rule.
pub mod layout {
    use std::fmt;

    /// The page the kernel maps in.
    pub const PAGE: usize = 4096;
    /// Segments an image may carry (the kernel's bound).
    pub const MAX_SEGMENTS: usize = 16;

    /// Why the kernel would refuse the image (`exec_vmo`, ADR-0080).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum LayoutError {
        /// Not an ELF64 little-endian file.
        NotElf64,
        /// The program headers do not lie inside the first page.
        HeadersBeyondFirstPage,
        /// More `PT_LOAD`s than the kernel's bound.
        TooManySegments(usize),
        /// A segment's file bytes run past the end of the file.
        Truncated { index: usize },
        /// `p_offset` or `p_vaddr` is not page-aligned.
        SegmentOffGrid { index: usize, offset: u64, vaddr: u64 },
        /// Two segments share a file page (it would be mapped at two addresses with two permissions).
        SegmentsShareFilePage { first: usize, second: usize },
        /// A writable and executable segment (W^X).
        WritableAndExecutable { index: usize },
        /// A read-only segment with `p_memsz` beyond its file pages (a zero tail would be a lazy
        /// page, which is never executable).
        ReadOnlyZeroTail { index: usize },
        /// `p_filesz > p_memsz`.
        FileSizeBeyondMemSize { index: usize },
    }

    impl fmt::Display for LayoutError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Self::NotElf64 => write!(f, "not an ELF64 little-endian image"),
                Self::HeadersBeyondFirstPage => {
                    write!(f, "the program headers do not lie inside the first page")
                }
                Self::TooManySegments(n) => write!(f, "{n} PT_LOAD segments (at most {MAX_SEGMENTS})"),
                Self::Truncated { index } => write!(f, "PT_LOAD {index} runs past the end of the file"),
                Self::SegmentOffGrid { index, offset, vaddr } => write!(
                    f,
                    "PT_LOAD {index} is off the page grid (offset 0x{offset:x}, vaddr 0x{vaddr:x}); link with -z separate-loadable-segments"
                ),
                Self::SegmentsShareFilePage { first, second } => {
                    write!(f, "PT_LOAD {first} and {second} share a file page")
                }
                Self::WritableAndExecutable { index } => {
                    write!(f, "PT_LOAD {index} is writable and executable")
                }
                Self::ReadOnlyZeroTail { index } => {
                    write!(f, "read-only PT_LOAD {index} has a zero tail beyond its file pages")
                }
                Self::FileSizeBeyondMemSize { index } => {
                    write!(f, "PT_LOAD {index} has p_filesz > p_memsz")
                }
            }
        }
    }

    impl std::error::Error for LayoutError {}

    /// What the kernel will do with the image.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Layout {
        /// `PT_LOAD`s with bytes.
        pub segments: usize,
        /// Pages mapped from the image's frames (text, rodata), shared by every process of it.
        pub shared_pages: usize,
        /// Pages copied per process (a writable segment's file pages).
        pub copied_pages: usize,
    }

    const PT_LOAD: u32 = 1;
    const PF_W: u32 = 2;
    const PF_X: u32 = 1;

    fn u16_at(b: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
    }
    fn u32_at(b: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
    }
    fn u64_at(b: &[u8], at: usize) -> Option<u64> {
        Some(u64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
    }
    fn pages(len: usize) -> usize {
        len.div_ceil(PAGE)
    }

    /// The kernel's decisions on `elf`, in the kernel's order.
    pub fn check(elf: &[u8]) -> Result<Layout, LayoutError> {
        if elf.len() < 64 || &elf[0..4] != b"\x7FELF" || elf[4] != 2 || elf[5] != 1 {
            return Err(LayoutError::NotElf64);
        }
        let phoff = u64_at(elf, 32).ok_or(LayoutError::NotElf64)? as usize;
        let phentsize = u16_at(elf, 54).ok_or(LayoutError::NotElf64)? as usize;
        let phnum = u16_at(elf, 56).ok_or(LayoutError::NotElf64)? as usize;
        let table_end = phnum
            .checked_mul(phentsize)
            .and_then(|n| n.checked_add(phoff))
            .ok_or(LayoutError::HeadersBeyondFirstPage)?;
        if phentsize < 56 || table_end > PAGE || table_end > elf.len() {
            return Err(LayoutError::HeadersBeyondFirstPage);
        }
        if phnum > MAX_SEGMENTS {
            return Err(LayoutError::TooManySegments(phnum));
        }
        let mut layout = Layout { segments: 0, shared_pages: 0, copied_pages: 0 };
        let mut file_ranges: Vec<(usize, usize, usize)> = Vec::new();
        for index in 0..phnum {
            let at = phoff + index * phentsize;
            let read = || -> Option<(u32, u32, u64, u64, u64, u64)> {
                Some((
                    u32_at(elf, at)?,
                    u32_at(elf, at + 4)?,
                    u64_at(elf, at + 8)?,
                    u64_at(elf, at + 16)?,
                    u64_at(elf, at + 32)?,
                    u64_at(elf, at + 40)?,
                ))
            };
            let (p_type, p_flags, p_offset, p_vaddr, p_filesz, p_memsz) =
                read().ok_or(LayoutError::HeadersBeyondFirstPage)?;
            if p_type != PT_LOAD || p_memsz == 0 {
                continue;
            }
            if p_filesz > p_memsz {
                return Err(LayoutError::FileSizeBeyondMemSize { index });
            }
            if p_offset % PAGE as u64 != 0 || p_vaddr % PAGE as u64 != 0 {
                return Err(LayoutError::SegmentOffGrid {
                    index,
                    offset: p_offset,
                    vaddr: p_vaddr,
                });
            }
            let (offset, filesz, memsz) = (p_offset as usize, p_filesz as usize, p_memsz as usize);
            if offset.checked_add(filesz).is_none_or(|end| end > elf.len()) {
                return Err(LayoutError::Truncated { index });
            }
            let writable = p_flags & PF_W != 0;
            if writable && p_flags & PF_X != 0 {
                return Err(LayoutError::WritableAndExecutable { index });
            }
            let file_pages = if filesz == 0 { 0 } else { pages(filesz) };
            if file_pages != 0 {
                let end = offset + file_pages * PAGE;
                if let Some(&(first, _, _)) =
                    file_ranges.iter().find(|&&(_, lo, hi)| offset < hi && lo < end)
                {
                    return Err(LayoutError::SegmentsShareFilePage { first, second: index });
                }
                file_ranges.push((index, offset, end));
            }
            if !writable {
                if file_pages == 0 || pages(memsz) != file_pages {
                    return Err(LayoutError::ReadOnlyZeroTail { index });
                }
                layout.shared_pages += file_pages;
            } else {
                layout.copied_pages += file_pages;
            }
            layout.segments += 1;
        }
        Ok(layout)
    }
}

/// RV64 instruction encoders the probe payloads are assembled with.
pub mod rv {
    pub fn push(out: &mut Vec<u8>, instr: u32) {
        out.extend_from_slice(&instr.to_le_bytes());
    }

    pub fn encode_auipc(rd: u8, imm20: i32) -> u32 {
        ((imm20 as u32) << 12) | ((rd as u32) << 7) | 0x17
    }

    pub fn encode_addi(rd: u8, rs1: u8, imm: i32) -> u32 {
        let imm_masked = (imm as u32) & 0xfff;
        (imm_masked << 20) | ((rs1 as u32) << 15) | ((rd as u32) << 7) | 0x13
    }

    pub fn encode_lui(rd: u8, imm20: i32) -> u32 {
        ((imm20 as u32) << 12) | ((rd as u32) << 7) | 0x37
    }

    pub fn encode_sb(rs2: u8, rs1: u8, imm: i32) -> u32 {
        let imm = imm as u32;
        let imm11_5 = (imm >> 5) & 0x7f;
        let imm4_0 = imm & 0x1f;
        (imm11_5 << 25) | ((rs2 as u32) << 20) | ((rs1 as u32) << 15) | (imm4_0 << 7) | 0x23
    }

    pub fn encode_lbu(rd: u8, rs1: u8, imm: i32) -> u32 {
        encode_i_type(0x03, rd, rs1, imm, 0b100)
    }

    pub fn encode_i_type(opcode: u32, rd: u8, rs1: u8, imm: i32, funct3: u32) -> u32 {
        let imm_masked = (imm as u32) & 0xfff;
        (imm_masked << 20) | ((rs1 as u32) << 15) | (funct3 << 12) | ((rd as u32) << 7) | opcode
    }

    pub fn encode_beq(rs1: u8, rs2: u8, offset: i32) -> u32 {
        encode_branch(0b000, rs1, rs2, offset)
    }

    pub fn encode_branch(funct3: u32, rs1: u8, rs2: u8, offset: i32) -> u32 {
        let off = offset as u32;
        let imm12 = (off >> 12) & 0x1;
        let imm10_5 = (off >> 5) & 0x3f;
        let imm4_1 = (off >> 1) & 0xf;
        let imm11 = (off >> 11) & 0x1;
        (imm12 << 31)
            | (imm10_5 << 25)
            | ((rs2 as u32) << 20)
            | ((rs1 as u32) << 15)
            | (funct3 << 12)
            | (imm4_1 << 8)
            | (imm11 << 7)
            | 0x63
    }

    pub fn encode_jal(rd: u8, offset: i32) -> u32 {
        let off = offset as u32;
        let imm20 = (off >> 20) & 0x1;
        let imm10_1 = (off >> 1) & 0x3ff;
        let imm11 = (off >> 11) & 0x1;
        let imm19_12 = (off >> 12) & 0xff;
        (imm20 << 31)
            | (imm10_1 << 21)
            | (imm11 << 20)
            | (imm19_12 << 12)
            | ((rd as u32) << 7)
            | 0x6f
    }

    pub fn encode_blt(rs1: u8, rs2: u8, offset: i32) -> u32 {
        encode_branch(0b100, rs1, rs2, offset)
    }

    pub fn encode_bne(rs1: u8, rs2: u8, offset: i32) -> u32 {
        encode_branch(0b001, rs1, rs2, offset)
    }

    /// `cbo.clean 0(rs1)`: MISC-MEM, funct3 010, rd 0, imm 1 (Zicbom).
    pub fn encode_cbo_clean(rs1: u8) -> u32 {
        encode_i_type(0x0f, 0, rs1, 1, 0b010)
    }
}

/// The tiny ELFs the probes are: one `PT_LOAD` (R|X) with the text on the page grid — file
/// offset [`tiny::TEXT_OFFSET`], address [`tiny::ENTRY`] — so the kernel maps it from the sealed
/// image's frame like any service's text.
pub mod tiny {
    use super::rv::*;

    /// The text's address and entry point.
    pub const ENTRY: u64 = 0x1000;
    /// The text's file offset (one page: the header and the one program header before it).
    pub const TEXT_OFFSET: usize = 0x1000;
    /// Inside [`print_and_exit`]'s text: the first instruction of its "done:" tail (`addi a0,
    /// x0, <code>`) — a probe that must end in a fault or a refused instruction replaces it.
    pub const DONE_OFFSET: usize = 0x20;

    /// An ELF64/RISC-V image around `text`, at the page grid.
    pub fn build_elf(text: &[u8]) -> Vec<u8> {
        let mut header = Vec::with_capacity(64);
        header.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        header.extend_from_slice(&2u16.to_le_bytes()); // ET_EXEC
        header.extend_from_slice(&243u16.to_le_bytes()); // EM_RISCV
        header.extend_from_slice(&1u32.to_le_bytes());
        header.extend_from_slice(&ENTRY.to_le_bytes());
        header.extend_from_slice(&(64u64).to_le_bytes()); // e_phoff
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
        ph.extend_from_slice(&1u32.to_le_bytes()); // PT_LOAD
        ph.extend_from_slice(&(0x5u32).to_le_bytes()); // R|X
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
        elf.resize(TEXT_OFFSET, 0);
        elf.extend_from_slice(text);
        while elf.len() % 4 != 0 {
            elf.push(0);
        }
        elf
    }

    /// A text that prints `msg` (NUL-terminated) through `debug_putc` and exits with
    /// `exit_code`: twelve instructions, the message at 0x30.
    pub fn print_and_exit(exit_code: i32, msg: &[u8]) -> Vec<u8> {
        const SYSCALL_DEBUG_PUTC: i32 = 16;
        const SYSCALL_EXIT: i32 = 11;
        const MSG_OFFSET: i32 = 0x30;
        const LABEL_LOOP: i32 = 0x08;
        const LABEL_DONE: i32 = DONE_OFFSET as i32;

        let mut text = Vec::new();
        let mut pc = 0i32;
        // s0 (x8) holds the message pointer across syscalls.
        push(&mut text, encode_auipc(8, 0));
        pc += 4;
        push(&mut text, encode_addi(8, 8, MSG_OFFSET));
        pc += 4;
        // loop: a0 = *(u8*)s0; if a0 == 0 goto done; debug_putc; s0 += 1; goto loop
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
        assert_eq!(pc, LABEL_DONE);
        push(&mut text, encode_addi(10, 0, exit_code));
        pc += 4;
        push(&mut text, encode_addi(17, 0, SYSCALL_EXIT));
        pc += 4;
        push(&mut text, 0x00000073); // ecall
        pc += 4;
        push(&mut text, 0x0000006f); // jal x0, 0 (never reached)
        pc += 4;
        assert_eq!(pc, MSG_OFFSET, "message offset mismatch");
        text.extend_from_slice(msg);
        text
    }
}

#[cfg(test)]
mod tests {
    use super::layout::{check, Layout, LayoutError, PAGE};
    use super::tiny;

    /// An image of `segs` = (flags, offset, vaddr, filesz, memsz) with `file_len` bytes.
    fn image(segs: &[(u32, u64, u64, u64, u64)], file_len: usize) -> Vec<u8> {
        let mut elf = vec![0u8; file_len];
        elf[0..7].copy_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1]);
        elf[32..40].copy_from_slice(&64u64.to_le_bytes());
        elf[54..56].copy_from_slice(&56u16.to_le_bytes());
        elf[56..58].copy_from_slice(&(segs.len() as u16).to_le_bytes());
        for (i, &(flags, off, va, filesz, memsz)) in segs.iter().enumerate() {
            let at = 64 + 56 * i;
            elf[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
            elf[at + 4..at + 8].copy_from_slice(&flags.to_le_bytes());
            elf[at + 8..at + 16].copy_from_slice(&off.to_le_bytes());
            elf[at + 16..at + 24].copy_from_slice(&va.to_le_bytes());
            elf[at + 32..at + 40].copy_from_slice(&filesz.to_le_bytes());
            elf[at + 40..at + 48].copy_from_slice(&memsz.to_le_bytes());
        }
        elf
    }

    const R: u32 = 4;
    const RX: u32 = 5;
    const RW: u32 = 6;

    #[test]
    fn a_tiny_probe_is_one_shared_page() {
        let elf = tiny::build_elf(&tiny::print_and_exit(0, b"child: x\n\0"));
        assert_eq!(check(&elf), Ok(Layout { segments: 1, shared_pages: 1, copied_pages: 0 }));
        assert_eq!(elf.len() % 4, 0);
        assert!(elf.len() > tiny::TEXT_OFFSET);
    }

    #[test]
    fn a_three_segment_service_shares_text_and_rodata_and_copies_its_data_page() {
        // lld with -z separate-loadable-segments: R at 0, RX on its own page, RW with a bss tail.
        let elf = image(
            &[
                (R, 0, 0x10000, 0x1800, 0x1800),
                (RX, 0x2000, 0x12000, 0x600, 0x600),
                (RW, 0x3000, 0x13000, 0x11, 0x5000),
            ],
            4 * PAGE,
        );
        assert_eq!(check(&elf), Ok(Layout { segments: 3, shared_pages: 3, copied_pages: 1 }));
    }

    #[test]
    fn test_reject_a_segment_off_the_page_grid() {
        let elf = image(&[(RX, 0x100, 0x1000, 0x40, 0x40)], PAGE);
        assert_eq!(
            check(&elf),
            Err(LayoutError::SegmentOffGrid { index: 0, offset: 0x100, vaddr: 0x1000 })
        );
    }

    #[test]
    fn test_reject_two_segments_on_one_file_page() {
        let elf = image(
            &[(R, 0x1000, 0x10000, 0x800, 0x800), (RX, 0x1000, 0x11000, 0x800, 0x800)],
            2 * PAGE,
        );
        assert_eq!(check(&elf), Err(LayoutError::SegmentsShareFilePage { first: 0, second: 1 }));
    }

    #[test]
    fn test_reject_writable_and_executable_and_a_read_only_zero_tail() {
        let wx = image(&[(7, 0x1000, 0x1000, 0x10, 0x10)], 2 * PAGE);
        assert_eq!(check(&wx), Err(LayoutError::WritableAndExecutable { index: 0 }));
        let tail = image(&[(R, 0x1000, 0x1000, 0x10, 0x3000)], 2 * PAGE);
        assert_eq!(check(&tail), Err(LayoutError::ReadOnlyZeroTail { index: 0 }));
    }

    #[test]
    fn test_reject_not_an_elf_or_truncated() {
        assert_eq!(check(b"nope"), Err(LayoutError::NotElf64));
        let short = image(&[(RX, 0x1000, 0x1000, 0x2000, 0x2000)], 2 * PAGE);
        assert_eq!(check(&short), Err(LayoutError::Truncated { index: 0 }));
    }
}
