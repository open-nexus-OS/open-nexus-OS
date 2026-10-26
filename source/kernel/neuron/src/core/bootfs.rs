// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! CONTEXT: the bootfs (ADR-0081, TASK-0333 6c-2). The boot image is one verified container —
//! the flat kernel and a bootfs, under one digest and one signature. The loader places the
//! bootfs past the kernel's memory footprint and names it in `/chosen/nexus,bootfs-start` and
//! `/chosen/nexus,bootfs-len`. This module reads that ONCE on the boot hart before the frame
//! pool is built (its pages are never the pool's), checks it lies in a memory bank past the
//! kernel's own image and clear of the tree, parses its table (`nexus-bootfs`, `NXFS0001`) and
//! adopts every file as a FIXED object born SEALED: no writer ever existed, the loader verified
//! the bytes, so every file is an image a process may run from (`exec::load_image`). File 0 is
//! init, spawned by the kernel through the same loader every other process goes through; init
//! receives the table page read-only and one capability per file in its fixed slots
//! (`task::init_caps`). The kernel never writes a byte of the bootfs and embeds no program.
//! OWNERS: @kernel-team @runtime
//! STATUS: Functional
//! PUBLIC API: capture_early(), range(), adopt(), init_file(), toc_alias(), file_aliases()
//! TEST_COVERAGE: `KSELFTEST: bootfs ok (files= bytes=)` on every QEMU lane and on the board;
//!   the codec's reject matrix in `nexus-bootfs`; the loader's placement in `nxboot`'s tests
//! ADR: docs/adr/0081-the-boot-image-is-one-verified-container-kernel-plus-bootfs-every-core-program-runs-from-it-no-program-embeds-another.md

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::cap::{Capability, CapabilityKind, Rights};
use crate::mm::vmo::VmoId;
use nexus_bootfs::toc::{Toc, MAX_FILES};

/// The page the table is laid out in (the codec's, as a `usize`).
const PAGE: usize = nexus_bootfs::PAGE as usize;
/// The loader's window is 56 MiB (RFC-0089 §2): no bootfs is larger.
const MAX_LEN: usize = 56 * 1024 * 1024;

static BOOTFS_PHYS: AtomicUsize = AtomicUsize::new(0);
static BOOTFS_LEN: AtomicUsize = AtomicUsize::new(0);

/// One file of the bootfs as an object.
#[derive(Clone, Copy, Debug)]
pub struct File {
    /// The sealed fixed object over the file's pages.
    pub vmo: VmoId,
    /// The object's length (whole pages).
    pub pages: usize,
    /// The stack the packer declared for it (pages).
    pub stack_pages: usize,
    /// The global pointer the packer read from its ELF.
    pub global_pointer: usize,
}

/// The adopted files, in table order, and the table page's own object.
struct Adopted {
    toc: Option<VmoId>,
    files: [Option<File>; MAX_FILES],
    count: usize,
}

static mut ADOPTED: Adopted = Adopted { toc: None, files: [None; MAX_FILES], count: 0 };

/// Reads where the loader put the bootfs from `/chosen`. Runs on the boot hart before the frame
/// pool is built; a bootfs that is absent or mis-described is left unrecorded here and reported
/// by `adopt` (the boot gate), never guessed at.
///
/// # Safety
/// Single-threaded early boot only (no other hart runs kernel code yet).
pub unsafe fn capture_early() {
    let chosen = crate::boot_fdt::bytes()
        .and_then(|b| nexus_fdt::Fdt::new(b).ok())
        .and_then(|f| f.chosen().ok());
    let Some(chosen) = chosen else { return };
    let (Some(start), Some(len)) =
        (chosen.nexus_u64("bootfs-start"), chosen.nexus_u64("bootfs-len"))
    else {
        return;
    };
    let (Ok(start), Ok(len)) = (usize::try_from(start), usize::try_from(len)) else { return };
    if start == 0 || len == 0 || start % PAGE != 0 || len % PAGE != 0 || len > MAX_LEN {
        return;
    }
    BOOTFS_PHYS.store(start, Ordering::Relaxed);
    BOOTFS_LEN.store(len, Ordering::Relaxed);
}

/// The bootfs's physical `[start, end)`, page-aligned — excluded from the frame pool.
pub fn range() -> Option<(usize, usize)> {
    let start = BOOTFS_PHYS.load(Ordering::Relaxed);
    let len = BOOTFS_LEN.load(Ordering::Relaxed);
    (start != 0 && len != 0).then(|| (start, start + len))
}

/// Why the bootfs could not become objects.
enum Fault {
    Absent,
    OutsideMemory,
    OverlapsImage,
    OverlapsTree,
    Table(nexus_bootfs::Error),
    Object,
}

impl core::fmt::Display for Fault {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Absent => f.write_str("no bootfs in /chosen"),
            Self::OutsideMemory => f.write_str("outside every memory bank"),
            Self::OverlapsImage => f.write_str("overlaps the kernel image"),
            Self::OverlapsTree => f.write_str("overlaps the device tree"),
            Self::Table(e) => write!(f, "table: {e}"),
            Self::Object => f.write_str("object table full"),
        }
    }
}

fn check_placement(start: usize, end: usize) -> Result<(), Fault> {
    let in_a_bank = crate::boot_fdt::bytes()
        .and_then(|b| nexus_fdt::Fdt::new(b).ok())
        .map(|f| {
            f.memory_banks().any(|b| {
                let (Ok(base), Ok(size)) = (usize::try_from(b.base), usize::try_from(b.size))
                else {
                    return false;
                };
                base <= start && base.checked_add(size).is_some_and(|bank_end| end <= bank_end)
            })
        })
        .unwrap_or(false);
    if !in_a_bank {
        return Err(Fault::OutsideMemory);
    }
    let (image_va, image_end_va) = crate::boot_image::range();
    let image_pa = crate::phys::virt_to_phys(image_va);
    let image_end_pa = image_pa + (image_end_va - image_va);
    if start < image_end_pa && image_pa < end {
        return Err(Fault::OverlapsImage);
    }
    if let Some((tree_start, tree_end)) = crate::boot_fdt::range() {
        if start < tree_end && tree_start < end {
            return Err(Fault::OverlapsTree);
        }
    }
    Ok(())
}

fn adopt_all(start: usize, len: usize) -> Result<(usize, usize), Fault> {
    // SAFETY: the loader's pages, inside a bank the direct map covers, never written by the
    // kernel; `len` is at least one page (checked at capture).
    let page =
        unsafe { core::slice::from_raw_parts(crate::phys::phys_to_virt(start) as *const u8, PAGE) };
    let toc = Toc::parse(page).map_err(Fault::Table)?;
    if toc.total_len() != len as u64 {
        return Err(Fault::Table(nexus_bootfs::Error::Layout));
    }
    let toc_vmo = crate::mm::vmo::adopt_sealed(start, PAGE).map_err(|_| Fault::Object)?;
    let mut files = [None; MAX_FILES];
    let mut count = 0usize;
    let mut bytes = 0usize;
    for (index, file) in toc.files().enumerate() {
        let (Ok(offset), Ok(file_len)) = (usize::try_from(file.offset), usize::try_from(file.len))
        else {
            return Err(Fault::Table(nexus_bootfs::Error::Layout));
        };
        let pages = file_len.div_ceil(PAGE) * PAGE;
        let vmo = crate::mm::vmo::adopt_sealed(start + offset, pages).map_err(|_| Fault::Object)?;
        files[index] = Some(File {
            vmo,
            pages,
            stack_pages: file.stack_pages.max(1) as usize,
            global_pointer: file.global_pointer as usize,
        });
        count += 1;
        bytes += file_len;
    }
    // SAFETY: the boot hart, before any task exists; written once.
    unsafe {
        core::ptr::addr_of_mut!(ADOPTED).write(Adopted { toc: Some(toc_vmo), files, count });
    }
    Ok((count, bytes))
}

/// The bootfs becomes objects: one sealed fixed object per file, one read-only object for the
/// table page. Runs once on the boot hart after the kernel address space is active, before any
/// task exists. A bootfs that is absent, misplaced or malformed is a boot gate: the rung reads
/// FAIL and the kernel stops — there is no init to spawn without it.
pub fn adopt() {
    let result = match range() {
        None => Err(Fault::Absent),
        Some((start, end)) => {
            check_placement(start, end).and_then(|()| adopt_all(start, end - start))
        }
    };
    match result {
        Ok((files, bytes)) => {
            log_info!(target: "selftest", "KSELFTEST: bootfs ok (files={} bytes={})", files, bytes);
        }
        Err(fault) => {
            log_error!(target: "selftest", "KSELFTEST: bootfs FAIL ({})", fault);
            panic!("boot gate: no bootfs");
        }
    }
}

fn adopted() -> &'static Adopted {
    // SAFETY: written once by `adopt` on the boot hart before any task exists; read-only after.
    unsafe { &*core::ptr::addr_of!(ADOPTED) }
}

/// File 0 — the program the kernel spawns (init).
pub fn init_file() -> Option<File> {
    adopted().files[0]
}

/// The read-only capability of the table page init receives (`INIT_BOOTFS_TOC_SLOT`).
pub fn toc_alias() -> Option<Capability> {
    let id = adopted().toc?;
    Some(Capability { kind: CapabilityKind::VmoRo { id, len: PAGE }, rights: Rights::MAP })
}

/// Every file's read-only capability with its table index (`INIT_BOOTFS_FILES_SLOT + index`).
pub fn file_aliases() -> impl Iterator<Item = (usize, Capability)> {
    let a = adopted();
    a.files[..a.count].iter().enumerate().filter_map(|(index, file)| {
        let file = (*file)?;
        Some((
            index,
            Capability {
                kind: CapabilityKind::VmoRo { id: file.vmo, len: file.pages },
                rights: Rights::MAP,
            },
        ))
    })
}
