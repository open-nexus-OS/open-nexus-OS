//! CONTEXT: Integration tests for nexus-loader ELF64/RISC-V loading functionality
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 3 integration tests
//!
//! TEST_SCOPE:
//!   - ELF64/RISC-V parsing and validation
//!   - Segment mapping and ordering
//!   - Security constraint enforcement
//!   - Loader integration with mapper interface
//!
//! TEST_SCENARIOS:
//!   - test_parse_fixture_segments_are_sorted(): Verify segments are sorted by virtual address
//!   - test_load_with_invokes_mapper_in_order(): Verify mapper is called in correct order
//!   - test_rejects_write_execute_segments(): Verify W+X segments are rejected
//!
//! DEPENDENCIES:
//!   - nexus_loader::parse_elf64_riscv: ELF parsing functionality
//!   - nexus_loader::load_with: Loading functionality
//!   - RecordingMapper: Test mapper implementation
//!   - Test ELF fixture data
//!
//! ADR: docs/adr/0002-nexus-loader-architecture.md

use byteorder::{LittleEndian, WriteBytesExt};
use nexus_loader::{load_with, parse_elf64_riscv, Error, Mapper, Prot, SegmentPlan};

struct RecordingMapper {
    segments: Vec<(SegmentPlan, Vec<u8>)>,
}

impl RecordingMapper {
    fn new() -> Self {
        Self {
            segments: Vec::new(),
        }
    }
}

impl Mapper for RecordingMapper {
    fn map_segment(&mut self, seg: &SegmentPlan, src: &[u8]) -> Result<(), Error> {
        self.segments.push((seg.clone(), src.to_vec()));
        Ok(())
    }
}

fn fixture() -> &'static [u8] {
    let mut buf = Vec::new();
    let mut e_ident = [0u8; 16];
    e_ident[0] = 0x7f;
    e_ident[1] = b'E';
    e_ident[2] = b'L';
    e_ident[3] = b'F';
    e_ident[4] = goblin::elf::header::ELFCLASS64;
    e_ident[5] = goblin::elf::header::ELFDATA2LSB;
    e_ident[6] = 1;
    buf.extend_from_slice(&e_ident);
    buf.extend_from_slice(&[0u8; 48]);
    let mut cursor = std::io::Cursor::new(buf);
    cursor.set_position(16);
    cursor.write_u16::<LittleEndian>(2).unwrap(); // e_type
    cursor
        .write_u16::<LittleEndian>(goblin::elf::header::EM_RISCV)
        .unwrap();
    cursor.write_u32::<LittleEndian>(1).unwrap(); // e_version
    cursor.write_u64::<LittleEndian>(0x10000).unwrap(); // e_entry
    cursor.write_u64::<LittleEndian>(0x40).unwrap(); // e_phoff
    cursor.write_u64::<LittleEndian>(0).unwrap(); // e_shoff
    cursor.write_u32::<LittleEndian>(0).unwrap(); // e_flags
    cursor.write_u16::<LittleEndian>(64).unwrap(); // e_ehsize
    cursor.write_u16::<LittleEndian>(56).unwrap(); // e_phentsize
    cursor.write_u16::<LittleEndian>(2).unwrap(); // e_phnum
    cursor.write_u16::<LittleEndian>(0).unwrap(); // e_shentsize
    cursor.write_u16::<LittleEndian>(0).unwrap(); // e_shnum
    cursor.write_u16::<LittleEndian>(0).unwrap(); // e_shstrndx
    cursor.set_position(64);
    // First PT_LOAD RX segment
    cursor
        .write_u32::<LittleEndian>(goblin::elf::program_header::PT_LOAD)
        .unwrap();
    cursor
        .write_u32::<LittleEndian>(
            goblin::elf::program_header::PF_R | goblin::elf::program_header::PF_X,
        )
        .unwrap();
    cursor.write_u64::<LittleEndian>(0x100).unwrap(); // p_offset
    cursor.write_u64::<LittleEndian>(0x10000).unwrap(); // p_vaddr
    cursor.write_u64::<LittleEndian>(0x10000).unwrap(); // p_paddr
    cursor.write_u64::<LittleEndian>(0x20).unwrap(); // p_filesz
    cursor.write_u64::<LittleEndian>(0x20).unwrap(); // p_memsz
    cursor
        .write_u64::<LittleEndian>(nexus_loader::PAGE_SIZE)
        .unwrap(); // p_align
    cursor
        .write_u32::<LittleEndian>(goblin::elf::program_header::PT_LOAD)
        .unwrap();
    cursor
        .write_u32::<LittleEndian>(
            goblin::elf::program_header::PF_R | goblin::elf::program_header::PF_W,
        )
        .unwrap();
    cursor.write_u64::<LittleEndian>(0x200).unwrap(); // p_offset
    cursor.write_u64::<LittleEndian>(0x20000).unwrap(); // p_vaddr
    cursor.write_u64::<LittleEndian>(0x20000).unwrap(); // p_paddr
    cursor.write_u64::<LittleEndian>(0x10).unwrap(); // p_filesz
    cursor.write_u64::<LittleEndian>(0x20).unwrap(); // p_memsz
    cursor
        .write_u64::<LittleEndian>(nexus_loader::PAGE_SIZE)
        .unwrap(); // p_align
    let mut data = cursor.into_inner();
    let pad_len = 0x100 - data.len();
    data.extend(std::iter::repeat(0).take(pad_len));
    // Code bytes for first segment (dummy)
    data.extend_from_slice(&[0x13, 0x05, 0x00, 0x00]);
    data.resize(0x120, 0);
    let pad2 = 0x200 - data.len();
    data.extend(std::iter::repeat(0).take(pad2));
    data.extend_from_slice(&[1u8; 0x10]);
    Box::leak(data.into_boxed_slice())
}

#[test]
fn parse_fixture_segments_are_sorted() {
    let plan = parse_elf64_riscv(fixture()).expect("parse fixture");
    assert_eq!(plan.entry, 0x10000);
    assert_eq!(plan.segments.len(), 2);
    assert!(plan.segments[0].vaddr < plan.segments[1].vaddr);
    assert_eq!(plan.segments[0].prot, Prot::R | Prot::X);
    assert_eq!(plan.segments[1].prot, Prot::R | Prot::W);
}

#[test]
fn load_with_invokes_mapper_in_order() {
    let mut mapper = RecordingMapper::new();
    let plan = load_with(fixture(), &mut mapper).expect("load fixture");
    assert_eq!(mapper.segments.len(), plan.segments.len());
    let recorded_vaddrs: Vec<u64> = mapper.segments.iter().map(|(seg, _)| seg.vaddr).collect();
    let planned_vaddrs: Vec<u64> = plan.segments.iter().map(|seg| seg.vaddr).collect();
    assert_eq!(recorded_vaddrs, planned_vaddrs);
}

#[test]
fn rejects_write_execute_segments() {
    let mut bytes = fixture().to_vec();
    // Set PF_W | PF_X on the first program header
    let flags_offset = 68; // first program header flags
    let flags = u32::from_le_bytes(bytes[flags_offset..flags_offset + 4].try_into().unwrap());
    let new_flags = flags | 0b011;
    bytes[flags_offset..flags_offset + 4].copy_from_slice(&new_flags.to_le_bytes());
    let err = parse_elf64_riscv(&bytes).expect_err("should reject WX");
    assert_eq!(err, Error::ProtWx);
}
