// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Build-ID extraction for ELF inputs. Primary source is the
//! `.note.gnu.build-id` note (hex-encoded). Fallback (documented + tested):
//! `crash::deterministic_build_id(<file stem>)` — the exact function the OS
//! producer (execd) stamps into `MinidumpFrame.build_id` for payloads without
//! an embedded id, so index keys and dump keys always agree.
//! OWNERS: @reliability
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Unit tests below; integration in `tests/crashdump_v2_host`
//! ADR: tasks/TASK-0048-crashdump-v2a-host-pipeline-nxsym-nx-crash.md

use crate::NxsymError;
use object::Object;
use std::path::Path;

/// Where a Build-ID came from (kept for operator display + tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildIdSource {
    /// `.note.gnu.build-id` note embedded in the ELF.
    GnuNote,
    /// Deterministic fallback derived from the file stem via the `crash` crate.
    Fallback,
}

/// Deterministic fallback Build-ID for a binary name (delegates to the
/// `crash` crate so producer and indexer can never drift).
pub fn fallback_build_id(name: &str) -> String {
    crash::deterministic_build_id(name)
}

/// Extract the Build-ID for an ELF file already read into memory.
///
/// `name` is the file stem used for the fallback derivation.
pub fn build_id_for_elf(
    name: &str,
    elf_bytes: &[u8],
) -> Result<(String, BuildIdSource), NxsymError> {
    let file = object::File::parse(elf_bytes).map_err(|_| NxsymError::ElfParse)?;
    match file.build_id() {
        Ok(Some(raw)) if !raw.is_empty() => Ok((hex_lower(raw), BuildIdSource::GnuNote)),
        Ok(_) => Ok((fallback_build_id(name), BuildIdSource::Fallback)),
        Err(_) => Err(NxsymError::ElfParse),
    }
}

/// File stem used for fallback derivation (matches how the OS producer names
/// payloads: `demo.exit42.elf` → `demo.exit42`).
pub fn file_stem(path: &Path) -> String {
    match path.file_stem() {
        Some(stem) => stem.to_string_lossy().into_owned(),
        None => String::from("unknown"),
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

#[cfg(test)]
pub(crate) mod test_fixtures {
    /// Minimal 64-bit little-endian ELF with a single PT_NOTE segment carrying
    /// a GNU build-id note (desc = `id_bytes`). No section headers, so the
    /// `object` crate reads the note from the program header.
    pub fn minimal_elf_with_build_id(id_bytes: &[u8]) -> Vec<u8> {
        let ehsize = 64u64;
        let phentsize = 56u64;
        let note_off = ehsize + phentsize;
        // Note: namesz(4) descsz(4) type(4) name "GNU\0" desc (4-aligned).
        let mut note = Vec::new();
        note.extend_from_slice(&4u32.to_le_bytes());
        note.extend_from_slice(&(id_bytes.len() as u32).to_le_bytes());
        note.extend_from_slice(&3u32.to_le_bytes()); // NT_GNU_BUILD_ID
        note.extend_from_slice(b"GNU\0");
        note.extend_from_slice(id_bytes);
        while note.len() % 4 != 0 {
            note.push(0);
        }

        let mut elf = Vec::new();
        // e_ident
        elf.extend_from_slice(&[0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        elf.extend_from_slice(&2u16.to_le_bytes()); // e_type = EXEC
        elf.extend_from_slice(&0xF3u16.to_le_bytes()); // e_machine = RISC-V
        elf.extend_from_slice(&1u32.to_le_bytes()); // e_version
        elf.extend_from_slice(&0u64.to_le_bytes()); // e_entry
        elf.extend_from_slice(&ehsize.to_le_bytes()); // e_phoff
        elf.extend_from_slice(&0u64.to_le_bytes()); // e_shoff
        elf.extend_from_slice(&0u32.to_le_bytes()); // e_flags
        elf.extend_from_slice(&(ehsize as u16).to_le_bytes()); // e_ehsize
        elf.extend_from_slice(&(phentsize as u16).to_le_bytes()); // e_phentsize
        elf.extend_from_slice(&1u16.to_le_bytes()); // e_phnum
        elf.extend_from_slice(&0u16.to_le_bytes()); // e_shentsize
        elf.extend_from_slice(&0u16.to_le_bytes()); // e_shnum
        elf.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx
        debug_assert_eq!(elf.len(), ehsize as usize);

        // PT_NOTE program header.
        elf.extend_from_slice(&4u32.to_le_bytes()); // p_type = PT_NOTE
        elf.extend_from_slice(&4u32.to_le_bytes()); // p_flags = R
        elf.extend_from_slice(&note_off.to_le_bytes()); // p_offset
        elf.extend_from_slice(&0u64.to_le_bytes()); // p_vaddr
        elf.extend_from_slice(&0u64.to_le_bytes()); // p_paddr
        elf.extend_from_slice(&(note.len() as u64).to_le_bytes()); // p_filesz
        elf.extend_from_slice(&(note.len() as u64).to_le_bytes()); // p_memsz
        elf.extend_from_slice(&4u64.to_le_bytes()); // p_align
        debug_assert_eq!(elf.len(), note_off as usize);

        elf.extend_from_slice(&note);
        elf
    }

    /// Same minimal ELF but without any note segment (forces the fallback).
    pub fn minimal_elf_without_build_id() -> Vec<u8> {
        let mut elf = minimal_elf_with_build_id(&[0xAA]);
        // Rewrite the program header type to PT_NULL so no note is found.
        elf[64..68].copy_from_slice(&0u32.to_le_bytes());
        elf
    }
}

#[cfg(test)]
mod tests {
    use super::test_fixtures::{minimal_elf_with_build_id, minimal_elf_without_build_id};
    use super::*;

    #[test]
    fn test_build_id_from_gnu_note() {
        let elf = minimal_elf_with_build_id(&[0xDE, 0xAD, 0xBE, 0xEF]);
        let (id, source) = build_id_for_elf("demo", &elf).expect("extract");
        assert_eq!(id, "deadbeef");
        assert_eq!(source, BuildIdSource::GnuNote);
    }

    #[test]
    fn test_build_id_fallback_matches_producer_derivation() {
        let elf = minimal_elf_without_build_id();
        let (id, source) = build_id_for_elf("demo.exit42", &elf).expect("extract");
        assert_eq!(source, BuildIdSource::Fallback);
        assert_eq!(id, crash::deterministic_build_id("demo.exit42"));
    }

    #[test]
    fn test_reject_non_elf_input() {
        assert_eq!(build_id_for_elf("x", b"not an elf"), Err(NxsymError::ElfParse));
    }

    #[test]
    fn test_build_id_extraction_is_deterministic() {
        let elf = minimal_elf_with_build_id(&[0x01, 0x02]);
        let a = build_id_for_elf("demo", &elf).expect("a");
        let b = build_id_for_elf("demo", &elf).expect("b");
        assert_eq!(a, b);
    }
}
