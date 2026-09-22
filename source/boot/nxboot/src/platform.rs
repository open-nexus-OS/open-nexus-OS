// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nxboot's ownership of `/chosen/nexus,*` (RFC-0098 C2, ADR-0066,
//! TASK-0244 P3). The tree the firmware handed over in `a1` is copied into a
//! buffer with headroom (the firmware's copy has none on QEMU; the FIT's copy on
//! the board has `dtc -p` padding — copying is the one path for both), the
//! loader's decisions are written into `/chosen` — the boot slot, the measured
//! handoff record's address, and on QEMU the lane's profile + display request
//! read from fw_cfg — and the COPY is what the kernel receives in `a1`. From here
//! on the kernel has ONE source: the tree. fw_cfg is read only in this file.
//!
//! OWNERS: @runtime @security
//! STATUS: Functional
//! API_STABILITY: Internal (the `/chosen` keys are RFC-0098's contract)
//! TEST_COVERAGE: QEMU ladder (`nxboot: fdt ok (` in every profile; the kernel's
//!   `KSELFTEST: platform from fdt ok` reads the copy back); parser host tests in nexus-fdt
//! ADR: docs/adr/0066-boot-chain-on-hardware-nxboot-as-fit-payload.md

extern crate alloc;

use alloc::vec::Vec;

use nexus_fdt::{ChosenWriter, Fdt};

use crate::arch;

/// Headroom reserved for the `/chosen` writes (four short properties + names).
const CHOSEN_HEADROOM: usize = 2048;
/// No stage we boot from produces a tree this large (QEMU virt ≈ 12 KiB, the
/// board ≈ 9 KiB); a bigger claim is a corrupt header, not a tree.
const MAX_DTB_LEN: usize = 1024 * 1024;
const FDT_MAGIC: u32 = 0xd00d_feed;

/// QEMU virt fw_cfg MMIO window: data register at +0, selector at +8.
const FW_CFG_BASE: usize = 0x1010_0000;
const FW_CFG_FILE_DIR: u16 = 0x19;
const FW_CFG_KEY_PROFILE: &[u8] = b"opt/org.open-nexus/selftest-profile";
const FW_CFG_KEY_DISPLAY: &[u8] = b"opt/org.open-nexus/display-mode";

/// Copy the tree, write `/chosen/nexus,*`, return the copy's address for `a1`.
/// A tree that does not parse is a loud reset (RFC-0098: never a wild read,
/// never a silent default).
pub fn prepare_dtb(dtb: usize, slot: char) -> usize {
    if dtb == 0 || dtb % 4 != 0 {
        fail("no tree in a1");
    }
    // The header first, byte by byte through the MMIO/RAM accessor: magic and
    // the tree's own size claim bound everything that follows.
    let mut header = [0u8; 8];
    for (i, b) in header.iter_mut().enumerate() {
        *b = arch::read_u8(dtb + i);
    }
    let magic = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let total = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if magic != FDT_MAGIC || !(40..=MAX_DTB_LEN).contains(&total) {
        fail("bad header in a1");
    }

    let src = arch::phys_slice(dtb, total);
    let mut buf: Vec<u8> = Vec::with_capacity(total + CHOSEN_HEADROOM);
    buf.extend_from_slice(src);
    buf.resize(total + CHOSEN_HEADROOM, 0);

    let mut w = match ChosenWriter::new(&mut buf) {
        Ok(w) => w,
        Err(e) => fail(error_str(e)),
    };
    let slot_buf = [slot as u8];
    let slot_str = core::str::from_utf8(&slot_buf).unwrap_or("?");
    let mut ok = w.set_nexus_str("boot-slot", slot_str).is_ok();
    ok &= w.set_nexus_u64("boot-record", bootfmt::handoff::ADDR as u64).is_ok();
    // QEMU lane knobs: absent on the board, absent when fw_cfg has no such file.
    let mut tmp = [0u8; 64];
    if let Some(n) = fw_cfg_read(FW_CFG_KEY_PROFILE, &mut tmp) {
        if let Ok(s) = core::str::from_utf8(&tmp[..n]) {
            ok &= w.set_nexus_str("boot-profile", s.trim_end_matches(['\0', '\n'])).is_ok();
        }
    }
    if let Some(n) = fw_cfg_read(FW_CFG_KEY_DISPLAY, &mut tmp) {
        if let Ok(s) = core::str::from_utf8(&tmp[..n]) {
            ok &= w.set_nexus_str("display-mode", s.trim_end_matches(['\0', '\n'])).is_ok();
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

/// Read a named fw_cfg file into `out`; `None` when there is no QEMU fw_cfg,
/// no such file, or the directory is implausible. Every caller treats "no
/// knob" identically, so the three are indistinguishable on purpose.
fn fw_cfg_read(name: &[u8], out: &mut [u8]) -> Option<usize> {
    let select = |key: u16| arch::write_u16(FW_CFG_BASE + 8, key.to_be());
    let read_u8 = || arch::read_u8(FW_CFG_BASE);
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
