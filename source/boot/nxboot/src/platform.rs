// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nxboot's reading of the device tree and its ownership of
//! `/chosen/nexus,*` (RFC-0098 C1/C2/C6, ADR-0066, TASK-0244 P3, TASK-0245 P2).
//! The tree the firmware handed over in `a1` is the loader's only source of
//! truth: the console it prints on (`/chosen/stdout-path`), the memory bank
//! and reserved ranges the kernel window is chosen from, the virtio transports
//! it probes for the boot disk, and — on QEMU only — the fw_cfg window whose
//! lane knobs it re-expresses in `/chosen`. Before the jump the tree is copied
//! into a buffer with headroom (the firmware's copy has none on QEMU; the FIT's
//! copy on the board has `dtc -p` padding — copying is the one path for both),
//! the loader's decisions are written into `/chosen` — the boot slot, the
//! measured handoff record, the QEMU knobs — and the COPY is what the kernel
//! receives in `a1`. No address in this crate is a constant.
//!
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Internal (the `/chosen` keys are RFC-0098's contract)
//! TEST_COVERAGE: QEMU ladder (`nxboot: fdt ok (` in every profile; the kernel's
//!   `KSELFTEST: platform from fdt ok` reads the copy back; `KSELFTEST: kernel
//!   image ok (base=…)` proves the window); parser host tests in nexus-fdt
//! ADR: docs/adr/0066-boot-chain-on-hardware-nxboot-as-fit-payload.md

extern crate alloc;

use alloc::vec::Vec;

use nexus_fdt::{ChosenWriter, Fdt};

use crate::arch;

/// Headroom reserved for the `/chosen` writes (the record + five short properties).
const CHOSEN_HEADROOM: usize = 2048;
/// No stage we boot from produces a tree this large (QEMU virt ≈ 12 KiB, the
/// board ≈ 9 KiB); a bigger claim is a corrupt header, not a tree.
const MAX_DTB_LEN: usize = 1024 * 1024;
const FDT_MAGIC: u32 = 0xd00d_feed;
/// The kernel window starts on a 2 MiB boundary (superpage-aligned identity map).
const KERNEL_ALIGN: usize = 2 * 1024 * 1024;

/// fw_cfg MMIO layout: data register at +0, selector at +8.
const FW_CFG_FILE_DIR: u16 = 0x19;
const FW_CFG_KEY_MODE: &[u8] = b"opt/org.open-nexus/selftest-mode";
const FW_CFG_KEY_PROFILE: &[u8] = b"opt/org.open-nexus/selftest-profile";
const FW_CFG_KEY_DISPLAY: &[u8] = b"opt/org.open-nexus/display-mode";

/// The firmware's tree: parsed view + where it lies (excluded from the window).
#[derive(Clone, Copy)]
pub struct Tree {
    pub fdt: Fdt<'static>,
    pub phys: usize,
    pub len: usize,
}

/// Validate the tree in `a1`, bring the console up from it, keep the view.
/// A tree that does not parse is a loud reset (RFC-0098: never a wild read,
/// never a silent default) — loud only once the console is known; before that
/// the reset itself is the signal.
pub fn init(dtb: usize) -> Tree {
    if dtb == 0 || dtb % 4 != 0 {
        fail("no tree in a1");
    }
    // The header first, byte by byte: magic and the tree's own size claim
    // bound everything that follows.
    let mut header = [0u8; 8];
    for (i, b) in header.iter_mut().enumerate() {
        *b = arch::read_u8(dtb + i);
    }
    let magic = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let total = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if magic != FDT_MAGIC || !(40..=MAX_DTB_LEN).contains(&total) {
        fail("bad header in a1");
    }
    let fdt = match Fdt::new(arch::phys_slice(dtb, total)) {
        Ok(f) => f,
        Err(e) => fail(error_str(e)),
    };
    // The console (RFC-0098 C3): base / `reg-shift` / `reg-io-width` of the
    // node `/chosen/stdout-path` names. Without one the loader stays silent
    // and the reset is the only signal — a board without a console is a
    // measurement error, not a boot.
    let Some(uart) = fdt.stdout().and_then(|n| n.reg(0).ok().flatten()) else {
        fail("no console in the tree");
    };
    let node = fdt.stdout().unwrap_or_else(|| fail("no console in the tree"));
    let shift = node.prop_u32("reg-shift").unwrap_or(0) as usize;
    let width = node.prop_u32("reg-io-width").unwrap_or(1) as usize;
    arch::set_console(uart.addr as usize, shift, width);
    Tree { fdt, phys: dtb, len: total }
}

/// Where the kernel goes (RFC-0098 C6): the lowest 2 MiB-aligned window of
/// `LOAD_MAX` bytes inside the lowest memory bank that avoids every reserved
/// range (`/reserved-memory` and the memreserve block), this image and the
/// tree. `None` = the bank cannot hold the slot budget — terminal.
pub fn kernel_window(t: &Tree) -> Option<(usize, usize)> {
    let bank = t.fdt.memory_banks().min_by_key(|b| b.base)?;
    let bank_start = usize::try_from(bank.base).ok()?;
    let bank_end = bank_start.checked_add(usize::try_from(bank.size).ok()?)?;
    let mut start = align_up(bank_start, KERNEL_ALIGN)?;
    // Each round jumps past one blocker; the blockers are finite, the bound is
    // the wait-loop doctrine's ceiling, not a search limit anyone reaches.
    for _ in 0..64 {
        let end = start.checked_add(arch::LOAD_MAX)?;
        if end > bank_end {
            return None;
        }
        match blockers(t).filter(|&(s, e)| s < end && e > start).map(|(_, e)| e).max() {
            None => return Some((start, arch::LOAD_MAX)),
            Some(past) => start = align_up(past, KERNEL_ALIGN)?,
        }
    }
    None
}

/// Every `[start, end)` the kernel window must not touch.
fn blockers(t: &Tree) -> impl Iterator<Item = (usize, usize)> + '_ {
    let reserved = t.fdt.reserved_ranges().filter_map(|r| span(r.base, r.size));
    let memreserve = t.fdt.reserved_entries().filter_map(|r| span(r.address, r.size));
    let tree = core::iter::once((t.phys, t.phys + t.len));
    let image = core::iter::once(arch::image_range());
    reserved.chain(memreserve).chain(tree).chain(image)
}

fn span(base: u64, size: u64) -> Option<(usize, usize)> {
    let s = usize::try_from(base).ok()?;
    let e = s.checked_add(usize::try_from(size).ok()?)?;
    Some((s, e))
}

fn align_up(v: usize, align: usize) -> Option<usize> {
    v.checked_add(align - 1).map(|x| x & !(align - 1))
}

/// The MMIO bases of every `virtio,mmio` transport the tree lists, for the
/// disk probe (RFC-0098 C1: devices by compatible, never by a window scan).
pub fn virtio_mmio_bases(t: &Tree) -> impl Iterator<Item = usize> + '_ {
    t.fdt
        .find_compatible(&["virtio,mmio"])
        .filter_map(|n| n.reg(0).ok().flatten())
        .filter_map(|r| usize::try_from(r.addr).ok())
}

/// Copy the tree, write `/chosen/nexus,*` (the slot, the measured record and
/// the QEMU lane knobs), return the copy's address for `a1`.
pub fn prepare_dtb(t: &Tree, slot: char, record: &[u8]) -> usize {
    let src = arch::phys_slice(t.phys, t.len);
    let mut buf: Vec<u8> = Vec::with_capacity(t.len + CHOSEN_HEADROOM);
    buf.extend_from_slice(src);
    buf.resize(t.len + CHOSEN_HEADROOM, 0);

    let mut w = match ChosenWriter::new(&mut buf) {
        Ok(w) => w,
        Err(e) => fail(error_str(e)),
    };
    let slot_buf = [slot as u8];
    let slot_str = core::str::from_utf8(&slot_buf).unwrap_or("?");
    let mut ok = w.set_nexus_str("boot-slot", slot_str).is_ok();
    ok &= w.set_nexus_bytes("boot-record", record).is_ok();
    // QEMU lane knobs: absent on the board (no fw_cfg node), absent when
    // fw_cfg has no such file.
    if let Some(fw) = fw_cfg_base(t) {
        let mut tmp = [0u8; 64];
        if let Some(n) = fw_cfg_read(fw, FW_CFG_KEY_MODE, &mut tmp) {
            if let Ok(s) = core::str::from_utf8(&tmp[..n]) {
                ok &= w.set_nexus_str("boot-mode", s.trim_end_matches(['\0', '\n'])).is_ok();
            }
        }
        if let Some(n) = fw_cfg_read(fw, FW_CFG_KEY_PROFILE, &mut tmp) {
            if let Ok(s) = core::str::from_utf8(&tmp[..n]) {
                ok &= w.set_nexus_str("boot-profile", s.trim_end_matches(['\0', '\n'])).is_ok();
            }
        }
        if let Some(n) = fw_cfg_read(fw, FW_CFG_KEY_DISPLAY, &mut tmp) {
            if let Ok(s) = core::str::from_utf8(&tmp[..n]) {
                ok &= w.set_nexus_str("display-mode", s.trim_end_matches(['\0', '\n'])).is_ok();
            }
        }
    }
    if !ok {
        fail("chosen write failed");
    }

    // Read the copy back through the parser: the marker carries values the
    // kernel will read from this very buffer.
    let (harts, tb) = match Fdt::new(&buf).and_then(|f| f.cpus()) {
        Ok(cpus) => (cpus.count(), cpus.timebase_hz),
        Err(e) => fail(error_str(e)),
    };
    arch::uart_puts(&alloc::format!("nxboot: fdt ok (harts={harts} tb={tb}Hz slot={slot})\n"));
    let ptr = buf.as_ptr() as usize;
    // The buffer must outlive this program: it lives in the loader's bump arena,
    // which nothing frees, and the kernel maps it read-only from `a1`.
    core::mem::forget(buf);
    ptr
}

fn fail(reason: &str) -> ! {
    arch::uart_puts("nxboot: PANIC (fdt: ");
    arch::uart_puts(reason);
    arch::uart_puts(")\n");
    arch::system_reset()
}

fn error_str(e: nexus_fdt::Error) -> &'static str {
    match e {
        nexus_fdt::Error::BadMagic => "bad magic",
        nexus_fdt::Error::Truncated => "truncated",
        nexus_fdt::Error::Version => "version",
        nexus_fdt::Error::Layout => "layout",
        nexus_fdt::Error::NoHeadroom => "no headroom",
        nexus_fdt::Error::NotFound => "no /chosen or /cpus",
        _ => "invalid",
    }
}

/// The QEMU fw_cfg MMIO window, if the tree lists one (`qemu,fw-cfg-mmio`).
fn fw_cfg_base(t: &Tree) -> Option<usize> {
    let node = t.fdt.find_compatible(&["qemu,fw-cfg-mmio"]).next()?;
    let reg = node.reg(0).ok().flatten()?;
    usize::try_from(reg.addr).ok()
}

/// Read a named fw_cfg file into `out`; `None` when the window is not fw_cfg,
/// there is no such file, or the directory is implausible. Every caller treats
/// "no knob" identically, so the three are indistinguishable on purpose.
fn fw_cfg_read(base: usize, name: &[u8], out: &mut [u8]) -> Option<usize> {
    let select = |key: u16| arch::write_u16(base + 8, key.to_be());
    let read_u8 = || arch::read_u8(base);
    let read_u16 = || u16::from_be_bytes([read_u8(), read_u8()]);
    let read_u32 = || u32::from_be_bytes([read_u8(), read_u8(), read_u8(), read_u8()]);

    select(0);
    let mut sig = [0u8; 4];
    for b in &mut sig {
        *b = read_u8();
    }
    if sig != *b"QEMU" {
        return None;
    }
    select(FW_CFG_FILE_DIR);
    let count = read_u32();
    if count > 128 {
        return None;
    }
    let mut found: Option<(u16, u32)> = None;
    for _ in 0..count {
        let size = read_u32();
        let key = read_u16();
        let _reserved = read_u16();
        let mut entry = [0u8; 56];
        for b in &mut entry {
            *b = read_u8();
        }
        let matches = name.len() <= entry.len()
            && entry[..name.len()] == *name
            && matches!(entry.get(name.len()).copied(), None | Some(0));
        if matches {
            found = Some((key, size));
            break;
        }
    }
    let (key, size) = found?;
    let len = out.len().min(size as usize);
    select(key);
    for b in &mut out[..len] {
        *b = read_u8();
    }
    Some(len)
}
