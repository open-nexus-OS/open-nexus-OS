// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! CONTEXT: `nx image payload` (ADR-0081) — the boot payload the slot carries and the NXBD
//! signs: one header sector, the flat kernel, the bootfs. The kernel item is the ELF's
//! `PT_LOAD` bytes laid out at their addresses (the static PIE links at 0, so address = offset
//! — byte for byte what `objcopy -O binary` produced before); the footprint is the last
//! segment's memory end, so the loader places the bootfs beyond everything the kernel zeroes.
//! The bootfs holds every program the manifest names — checked against the image rule the
//! kernel applies (`exec_image::layout`), its global pointer derived the way init-lite's build
//! script did (the symbol, else `.sdata + 0x800`), its stack from the manifest. Deterministic:
//! the same inputs give the same bytes.
//! OWNERS: @tools-team @kernel-team
//! STATUS: Functional
//! API_STABILITY: Unstable (the layouts are `nexus-bootfs`'s)
//! TEST_COVERAGE: unit tests below (flattening, footprint, the manifest rejects); every QEMU
//!   lane and board image is built through this command
//! ADR: docs/adr/0081-the-boot-image-is-one-verified-container-kernel-plus-bootfs-every-core-program-runs-from-it-no-program-embeds-another.md

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::cli_image::ImagePayloadArgs;
use crate::commands::image::hex;
use crate::error::{ExecResult, ExitClass, NxError};

/// What the build names: the programs of the bootfs, in spawn order.
#[derive(Deserialize)]
struct Manifest {
    files: Vec<ManifestFile>,
}

#[derive(Deserialize)]
struct ManifestFile {
    name: String,
    path: PathBuf,
    #[serde(default = "default_stack_pages")]
    stack_pages: u32,
}

fn default_stack_pages() -> u32 {
    8
}

fn reject(msg: String) -> NxError {
    NxError::new(ExitClass::ValidationReject, msg)
}

fn read(path: &Path) -> Result<Vec<u8>, NxError> {
    std::fs::read(path).map_err(|err| {
        NxError::new(
            ExitClass::MissingDependency,
            format!("payload: read {}: {err}", path.display()),
        )
    })
}

/// The header of a slot image, or the reject: every disk assembler takes only a container
/// (ADR-0081) — a bare kernel would boot a loader that finds no bootfs.
pub(crate) fn container(bytes: &[u8]) -> Result<nexus_bootfs::payload::Header, NxError> {
    let header = nexus_bootfs::payload::decode(bytes)
        .map_err(|err| reject(format!("image: not a boot payload container ({err:?})")))?;
    if header.total_len() != Some(bytes.len() as u64) {
        return Err(reject(format!(
            "image: payload length {} is not the container's {:?}",
            bytes.len(),
            header.total_len()
        )));
    }
    Ok(header)
}

/// A second build of the same programs: the container re-composed with `trailer` appended to
/// the kernel item (its bytes land in memory the kernel zeroes at its start — the first bytes of
/// `.bss`, inside the footprint), the bootfs untouched. The fixtures' os-B: genuinely different
/// bytes, a genuinely different digest, still a container the loader places.
pub(crate) fn with_kernel_trailer(payload: &[u8], trailer: &[u8]) -> Result<Vec<u8>, NxError> {
    let h = container(payload)?;
    let (ko, kl) = (h.kernel_offset as usize, h.kernel_len as usize);
    let (bo, bl) = (h.bootfs_offset as usize, h.bootfs_len as usize);
    let mut kernel = payload[ko..ko + kl].to_vec();
    kernel.extend_from_slice(trailer);
    if (kernel.len() as u64) > h.kernel_footprint {
        return Err(reject("image: the trailer does not fit the kernel's footprint".into()));
    }
    nexus_bootfs::payload::compose(&kernel, h.kernel_footprint, &payload[bo..bo + bl])
        .map_err(|err| reject(format!("image: re-composing the payload ({err:?})")))
}

pub(crate) fn handle_payload(args: ImagePayloadArgs) -> ExecResult {
    let elf = read(&args.kernel_elf)?;
    let (kernel, footprint) = flatten_kernel(&elf)?;
    let manifest: Manifest = serde_json::from_slice(&read(&args.manifest)?)
        .map_err(|err| reject(format!("payload: manifest {}: {err}", args.manifest.display())))?;
    if manifest.files.is_empty() {
        return Err(reject("payload: the manifest names no file".into()));
    }
    let mut builder = nexus_bootfs::toc::Builder::new();
    let mut files = Vec::new();
    for f in &manifest.files {
        let bytes = read(&f.path)?;
        exec_image::layout::check(&bytes)
            .map_err(|err| reject(format!("payload: {} ({}): {err}", f.name, f.path.display())))?;
        let gp = global_pointer(&bytes)
            .map_err(|why| reject(format!("payload: {} ({}): {why}", f.name, f.path.display())))?;
        builder
            .add(&f.name, &bytes, f.stack_pages, gp)
            .map_err(|err| reject(format!("payload: bootfs file {}: {err}", f.name)))?;
        files.push(json!({
            "name": f.name, "len": bytes.len(), "stack_pages": f.stack_pages,
            "global_pointer": format!("{gp:#x}"),
        }));
    }
    let bootfs = builder.finish().map_err(|err| reject(format!("payload: bootfs: {err}")))?;
    let payload = nexus_bootfs::payload::compose(&kernel, footprint, &bootfs)
        .map_err(|err| reject(format!("payload: container: {err}")))?;
    if let Some(parent) = args.out.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&args.out, &payload).map_err(|err| {
        NxError::new(ExitClass::Internal, format!("payload: write {}: {err}", args.out.display()))
    })?;
    let sha = hex(&Sha256::digest(&payload));
    let data = json!({
        "out": args.out.display().to_string(),
        "payload_len": payload.len(),
        "sha256": sha,
        "kernel_len": kernel.len(),
        "kernel_footprint": footprint,
        "bootfs_len": bootfs.len(),
        "files": files,
    });
    Ok((
        ExitClass::Success,
        format!(
            "image: payload ok (kernel={} footprint={} bootfs={} files={} total={})",
            kernel.len(),
            footprint,
            bootfs.len(),
            manifest.files.len(),
            payload.len()
        ),
        args.json,
        Some(data),
    ))
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[at..at + 8]);
    u64::from_le_bytes(a)
}

/// The kernel item and its footprint from the kernel ELF: every `PT_LOAD`'s file bytes at its
/// virtual address (the image links at 0), the footprint the last segment's memory end rounded
/// up to a page.
pub(crate) fn flatten_kernel(elf: &[u8]) -> Result<(Vec<u8>, u64), NxError> {
    const PT_LOAD: u32 = 1;
    const PAGE: u64 = 4096;
    if elf.len() < 64 || &elf[0..4] != b"\x7FELF" || elf[4] != 2 || elf[5] != 1 {
        return Err(reject("payload: the kernel is not a little-endian ELF64".into()));
    }
    let phoff = u64_at(elf, 32) as usize;
    let phentsize = u16_at(elf, 54) as usize;
    let phnum = u16_at(elf, 56) as usize;
    if phentsize < 56 || phnum == 0 || phnum > 16 {
        return Err(reject(
            "payload: the kernel's program headers are not where an ELF64 keeps them".into(),
        ));
    }
    let mut flat_len = 0u64;
    let mut footprint = 0u64;
    let mut loads = Vec::new();
    for i in 0..phnum {
        let at = phoff
            .checked_add(i * phentsize)
            .ok_or_else(|| reject("payload: program headers overflow".into()))?;
        let ph = elf
            .get(at..at + 56)
            .ok_or_else(|| reject("payload: program headers truncated".into()))?;
        if u32_at(ph, 0) != PT_LOAD {
            continue;
        }
        let (offset, vaddr, filesz, memsz) =
            (u64_at(ph, 8), u64_at(ph, 16), u64_at(ph, 32), u64_at(ph, 40));
        if filesz > memsz {
            return Err(reject(format!(
                "payload: kernel segment {i}: file size beyond memory size"
            )));
        }
        let file_end =
            offset.checked_add(filesz).ok_or_else(|| reject("payload: segment overflow".into()))?;
        if file_end > elf.len() as u64 {
            return Err(reject(format!("payload: kernel segment {i}: bytes beyond the file")));
        }
        let mem_end =
            vaddr.checked_add(memsz).ok_or_else(|| reject("payload: segment overflow".into()))?;
        let flat_end =
            vaddr.checked_add(filesz).ok_or_else(|| reject("payload: segment overflow".into()))?;
        if flat_end > 64 * 1024 * 1024 {
            return Err(reject(format!(
                "payload: kernel segment {i}: not a static PIE linked at 0 ({vaddr:#x})"
            )));
        }
        flat_len = flat_len.max(flat_end);
        footprint = footprint.max(mem_end);
        loads.push((offset as usize, vaddr as usize, filesz as usize));
    }
    if loads.is_empty() {
        return Err(reject("payload: the kernel has no loadable segment".into()));
    }
    let mut flat = vec![0u8; flat_len as usize];
    for (offset, vaddr, filesz) in loads {
        flat[vaddr..vaddr + filesz].copy_from_slice(&elf[offset..offset + filesz]);
    }
    let footprint = footprint.div_ceil(PAGE) * PAGE;
    Ok((flat, footprint))
}

/// The RISC-V global pointer a program is entered with: its `__global_pointer$` symbol, else
/// `.sdata + 0x800` (the psABI convention; release artifacts are stripped).
pub(crate) fn global_pointer(elf: &[u8]) -> Result<u64, String> {
    use object::{Object, ObjectSection, ObjectSymbol};
    let file = object::File::parse(elf).map_err(|err| format!("parse ELF: {err}"))?;
    if let Some(addr) =
        file.symbols().find(|s| s.name().ok() == Some("__global_pointer$")).map(|s| s.address())
    {
        return Ok(addr);
    }
    file.section_by_name(".sdata")
        .map(|s| s.address().saturating_add(0x800))
        .ok_or_else(|| "no __global_pointer$ and no .sdata (cannot derive gp)".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A little ELF64 with the given `PT_LOAD`s: (offset, vaddr, filesz, memsz) and `body`
    /// bytes after the headers.
    fn elf_with(loads: &[(u64, u64, u64, u64)], body: &[u8]) -> Vec<u8> {
        let phoff = 64u64;
        let mut out = vec![0u8; 64 + loads.len() * 56];
        out[0..4].copy_from_slice(b"\x7FELF");
        out[4] = 2;
        out[5] = 1;
        out[32..40].copy_from_slice(&phoff.to_le_bytes());
        out[54..56].copy_from_slice(&56u16.to_le_bytes());
        out[56..58].copy_from_slice(&(loads.len() as u16).to_le_bytes());
        for (i, (offset, vaddr, filesz, memsz)) in loads.iter().enumerate() {
            let at = 64 + i * 56;
            out[at..at + 4].copy_from_slice(&1u32.to_le_bytes());
            out[at + 8..at + 16].copy_from_slice(&offset.to_le_bytes());
            out[at + 16..at + 24].copy_from_slice(&vaddr.to_le_bytes());
            out[at + 32..at + 40].copy_from_slice(&filesz.to_le_bytes());
            out[at + 40..at + 48].copy_from_slice(&memsz.to_le_bytes());
        }
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn the_kernel_item_is_the_segments_at_their_addresses_and_the_footprint_the_memory_end() {
        // text at 0 (16 B), data at 0x2000 (4 B of file, 0x5000 of memory).
        let headers = 64 + 2 * 56;
        let mut body = vec![0xAAu8; 16];
        body.extend_from_slice(&[1, 2, 3, 4]);
        let elf = elf_with(
            &[(headers as u64, 0, 16, 16), (headers as u64 + 16, 0x2000, 4, 0x5000)],
            &body,
        );
        let (flat, footprint) = flatten_kernel(&elf).unwrap();
        assert_eq!(flat.len(), 0x2004);
        assert!(flat[..16].iter().all(|&b| b == 0xAA));
        assert!(flat[16..0x2000].iter().all(|&b| b == 0));
        assert_eq!(&flat[0x2000..], &[1, 2, 3, 4]);
        assert_eq!(footprint, 0x7000);
    }

    #[test]
    fn test_reject_kernel_that_is_not_a_pie_at_zero_or_lies_about_its_bytes() {
        let headers = 64 + 56;
        let elf = elf_with(&[(headers as u64, 0x8000_0000, 4, 4)], &[0; 4]);
        assert!(flatten_kernel(&elf).unwrap_err().message.contains("static PIE"));
        let elf = elf_with(&[(headers as u64, 0, 400, 400)], &[0; 4]);
        assert!(flatten_kernel(&elf).unwrap_err().message.contains("beyond the file"));
        let elf = elf_with(&[(headers as u64, 0, 8, 4)], &[0; 8]);
        assert!(flatten_kernel(&elf).unwrap_err().message.contains("beyond memory size"));
        assert!(flatten_kernel(b"not an elf").is_err());
    }

    #[test]
    fn a_tiny_program_has_a_global_pointer_or_is_refused_honestly() {
        let tiny = exec_image::tiny::build_elf(&exec_image::tiny::print_and_exit(0, b"x\n\0"));
        // The tiny ELF carries neither the symbol nor `.sdata`: the derivation says so.
        let why = global_pointer(&tiny).unwrap_err();
        assert!(why.contains("cannot derive gp"), "{why}");
    }

    #[test]
    fn a_build_variant_keeps_the_bootfs_and_changes_the_digest_inside_a_valid_container() {
        let mut files = nexus_bootfs::toc::Builder::new();
        files.add("init-lite", &[7u8; 3000], 16, 0x1800).unwrap();
        let bootfs = files.finish().unwrap();
        let base = nexus_bootfs::payload::compose(&[1u8; 1000], 1 << 20, &bootfs).unwrap();
        let variant = with_kernel_trailer(&base, &[0xAA; 512]).unwrap();
        let (hb, hv) = (container(&base).unwrap(), container(&variant).unwrap());
        assert_eq!(hv.kernel_len, hb.kernel_len + 512);
        assert_eq!(hv.kernel_footprint, hb.kernel_footprint);
        let bootfs_of = |h: nexus_bootfs::payload::Header, p: &[u8]| {
            p[h.bootfs_offset as usize..(h.bootfs_offset + h.bootfs_len) as usize].to_vec()
        };
        assert_eq!(bootfs_of(hb, &base), bootfs_of(hv, &variant));
        assert_ne!(base, variant);
        // Not a container, or a container whose bytes end elsewhere: refused.
        assert!(container(b"not a container at all").is_err());
        assert!(container(&base[..base.len() - 1]).is_err());
        let mut longer = base.clone();
        longer.extend_from_slice(&[0u8; 512]);
        assert!(container(&longer).is_err());
        // A trailer the footprint has no room for.
        let tight = nexus_bootfs::payload::compose(&[1u8; 1000], 1000, &bootfs).unwrap();
        assert!(with_kernel_trailer(&tight, &[0xAA; 512]).is_err());
    }
}
