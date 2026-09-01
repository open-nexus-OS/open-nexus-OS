// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Minimal QEMU `fw_cfg` MMIO reader (TASK-0289-B). The launcher
//! hands host-side boot configuration to the guest via `-fw_cfg` named
//! files; this module walks the file directory and copies ONE named file
//! out — the shared primitive behind runtime knobs that must be readable
//! WITHOUT a rebuild (selftest mode/profile, fault-fixture arming).
//! The caller owns the capability story: pass a cap slot holding the
//! fw_cfg MMIO window (create it via `device_mmio_cap_create` or receive
//! it via transfer) — this module only maps (once per slot argument, no
//! cache) and reads. Bounded by construction: ≤128 directory entries,
//! output clamped to the caller's buffer. Host builds return `None`.
//! OWNERS: @runtime
//! STATUS: Experimental (TASK-0289-B; parity source:
//!   selftest-client `os_lite/boot_cfg.rs`, which predates this module)
//! PUBLIC API: read_named_file()
//! TEST_COVERAGE: exercised live by every fw_cfg consumer lane; the
//!   selftest ladder fails loudly if the walk regresses.
//! ADR: docs/adr/0059-first-stage-boot-chain-nxboot-handoff.md

/// QEMU virt fw_cfg window base + the register layout inside the page:
/// data register at +0, selector at +8 (MMIO transport).
pub const FW_CFG_MMIO_BASE: usize = 0x1010_0000;
/// One page covers both registers.
pub const FW_CFG_MMIO_LEN: usize = 0x1000;

#[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
mod imp {
    const FW_CFG_FILE_DIR: u16 = 0x19;

    struct Regs {
        base: usize,
    }

    impl Regs {
        fn select(&self, select: u16) {
            unsafe { core::ptr::write_volatile((self.base + 8) as *mut u16, select.to_be()) };
        }
        fn read_u8(&self) -> u8 {
            unsafe { core::ptr::read_volatile(self.base as *const u8) }
        }
        fn read_be_u16(&self) -> u16 {
            u16::from_be_bytes([self.read_u8(), self.read_u8()])
        }
        fn read_be_u32(&self) -> u32 {
            u32::from_be_bytes([self.read_u8(), self.read_u8(), self.read_u8(), self.read_u8()])
        }
    }

    /// Maps the window behind `cap_slot` and copies the named file into
    /// `out` (clamped). `None` = map failed, no QEMU signature, or the
    /// file is absent — indistinguishable on purpose: every caller treats
    /// "no knob" identically.
    pub fn read_named_file(cap_slot: u32, name: &[u8], out: &mut [u8]) -> Option<usize> {
        let base = crate::mmio_map_auto(cap_slot, 0, super::FW_CFG_MMIO_LEN).ok()?;
        let regs = Regs { base };
        regs.select(0);
        let mut sig = [0u8; 4];
        for byte in &mut sig {
            *byte = regs.read_u8();
        }
        if sig != *b"QEMU" {
            return None;
        }
        regs.select(FW_CFG_FILE_DIR);
        let count = regs.read_be_u32();
        if count > 128 {
            return None;
        }
        let mut found: Option<(u16, u32)> = None;
        for _ in 0..count {
            let size = regs.read_be_u32();
            let select = regs.read_be_u16();
            let _reserved = regs.read_be_u16();
            let mut entry = [0u8; 56];
            for byte in &mut entry {
                *byte = regs.read_u8();
            }
            if entry_matches(&entry, name) {
                found = Some((select, size));
                break;
            }
        }
        let (select, size) = found?;
        let len = out.len().min(usize::try_from(size).ok()?);
        regs.select(select);
        for byte in &mut out[..len] {
            *byte = regs.read_u8();
        }
        Some(len)
    }

    fn entry_matches(actual: &[u8; 56], expected: &[u8]) -> bool {
        if expected.len() > actual.len() {
            return false;
        }
        actual[..expected.len()] == *expected
            && matches!(actual.get(expected.len()).copied(), None | Some(0))
    }
}

/// See the module header; the host arm reads nothing and returns `None`.
#[must_use]
pub fn read_named_file(cap_slot: u32, name: &[u8], out: &mut [u8]) -> Option<usize> {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    {
        imp::read_named_file(cap_slot, name, out)
    }
    #[cfg(not(all(nexus_env = "os", target_arch = "riscv64", target_os = "none")))]
    {
        let _ = (cap_slot, name, out);
        None
    }
}
