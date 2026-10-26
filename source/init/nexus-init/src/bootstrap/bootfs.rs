// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//! CONTEXT: init's view of the bootfs (ADR-0081, TASK-0333 6c-2). The kernel injects the
//! bootfs table page read-only into `INIT_BOOTFS_TOC_SLOT` and one sealed object per file into
//! `INIT_BOOTFS_FILES_SLOT + i`. This module maps the page once, parses it with `nexus-bootfs`
//! and answers the two questions init has: which core services to spawn — every file but init
//! itself and the launcher's hand-offs, in table order, with the stack pages and global
//! pointer the packer wrote — and which objects to hand the launcher at its spawn
//! (`nexus_service_topology::bootfs::HAND_OFFS`). No program's bytes are embedded anywhere:
//! every spawn execs an object the kernel made from the verified image.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU ladder — `init: start <svc>` for every core service, `execd` spawning
//!   app-hosts from the handed-over image; the parser is host-tested in nexus-bootfs

use core::sync::atomic::{AtomicUsize, Ordering};

use crate::os_payload::{InitError, Result, ServiceImage};
use nexus_abi::Rights;
use nexus_bootfs::toc::Toc;
use nexus_service_topology::bootfs::{HAND_OFFS, INIT_FILE};

const PAGE: usize = 4096;

static TOC_VA: AtomicUsize = AtomicUsize::new(0);

/// The table page, mapped read-only on first use. `None` = the kernel injected no table.
fn toc_page() -> Option<&'static [u8]> {
    let va = TOC_VA.load(Ordering::Acquire);
    if va != 0 {
        // SAFETY: mapped read-only below for exactly one page; never unmapped.
        return Some(unsafe { core::slice::from_raw_parts(va as *const u8, PAGE) });
    }
    let slot = nexus_abi::INIT_BOOTFS_TOC_SLOT;
    let mut info = nexus_abi::CapQuery::default();
    nexus_abi::cap_query(slot, &mut info).ok()?;
    if info.kind_tag != 3 || info.len != PAGE as u64 {
        return None;
    }
    let flags =
        nexus_abi::page_flags::VALID | nexus_abi::page_flags::READ | nexus_abi::page_flags::USER;
    let va = nexus_abi::vm_map(slot, 0, PAGE, flags).ok()?;
    TOC_VA.store(va, Ordering::Release);
    // SAFETY: one page mapped at `va`, read-only.
    Some(unsafe { core::slice::from_raw_parts(va as *const u8, PAGE) })
}

/// One file of the bootfs as init sees it: its name from the table, its object in init's slot.
#[derive(Clone, Copy)]
pub(crate) struct BootfsFile {
    pub(crate) name: &'static str,
    /// init's capability slot of the sealed object (`INIT_BOOTFS_FILES_SLOT + index`).
    pub(crate) slot: u32,
    pub(crate) stack_pages: u32,
    pub(crate) global_pointer: u64,
}

/// Every file of the table whose object the kernel injected, in table order. A file the table
/// names without an object in its slot is a kernel/ABI drift — refused loud, never skipped.
pub(crate) fn files() -> Result<alloc::vec::Vec<BootfsFile>> {
    let page = toc_page().ok_or(InitError::Elf("no bootfs table"))?;
    let toc = Toc::parse(page).map_err(|_| InitError::Elf("bootfs table"))?;
    let mut out = alloc::vec::Vec::with_capacity(toc.len());
    for (index, file) in toc.files().enumerate() {
        let slot = nexus_abi::INIT_BOOTFS_FILES_SLOT + index as u32;
        let mut info = nexus_abi::CapQuery::default();
        nexus_abi::cap_query(slot, &mut info).map_err(InitError::Abi)?;
        let pages = file.len.div_ceil(PAGE as u64) * PAGE as u64;
        if info.kind_tag != 3 || info.len != pages {
            return Err(InitError::Elf("bootfs file slot"));
        }
        out.push(BootfsFile {
            name: file.name,
            slot,
            stack_pages: file.stack_pages,
            global_pointer: file.global_pointer,
        });
    }
    Ok(out)
}

/// The core services init spawns, in table order: every file but init itself (file 0) and the
/// launcher's hand-offs. Leaked once — the table outlives every spawn and respawn.
pub(crate) fn service_images() -> Result<&'static [ServiceImage]> {
    let handed = |name: &str| HAND_OFFS.iter().any(|(n, _)| *n == name);
    let images: alloc::vec::Vec<ServiceImage> = files()?
        .into_iter()
        .filter(|f| f.name != INIT_FILE && !handed(f.name))
        .map(|f| ServiceImage {
            name: f.name,
            image: f.slot,
            stack_pages: u64::from(f.stack_pages),
            global_pointer: f.global_pointer,
        })
        .collect();
    Ok(alloc::boxed::Box::leak(images.into_boxed_slice()))
}

/// Hands the launcher its images (ADR-0081): every hand-off file's sealed object into execd's
/// declared slot, MAP rights, at its spawn — before it ever runs. A missing file is loud: the
/// launcher would answer UNSUPPORTED for every app, and that is a build defect, not a runtime
/// state.
pub(crate) fn hand_off_to_execd(pid: u32) -> Result<()> {
    let files = files()?;
    for (name, slot) in HAND_OFFS {
        let file = files.iter().find(|f| f.name == *name).ok_or(InitError::MissingElf)?;
        nexus_abi::cap_transfer_to_slot(pid, file.slot, Rights::MAP, *slot)
            .map_err(InitError::Abi)?;
    }
    Ok(())
}
