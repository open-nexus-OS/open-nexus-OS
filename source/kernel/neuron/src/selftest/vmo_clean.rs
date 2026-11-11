// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `KSELFTEST: vmo clean ok (` — ADR-0092's maintenance call at boot. A lazy object made
//! for a synthetic NON-coherent device: the frame the kernel commits and zeroes for it is cleaned
//! (`zeroed=` lines), `vmo_clean` over the object cleans its one committed page and skips the
//! absent one (`lines=`), and the refusals keep their identity — a read-only alias is EPERM, a
//! range past the end EINVAL — while an object for no device cleans nothing (`coherent=0`). With
//! no Zicbom in the tree (`block=0`) every count is honestly 0.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! TEST_COVERAGE: QEMU marker ladder (headless + full profiles), the board
//! ADR: docs/adr/0092-a-surface-is-a-buffer-the-app-draws-and-the-gpu-reads-gpud-allocates-it-for-its-owner-windowd-composes-by-reference.md

use super::*;
use crate::cap::CapError;
use crate::dma_reach::{DeviceDesc, DmaReach};
use crate::mm::vmo::clean::lines_cleaned;
use crate::syscall::{SYSCALL_VMO_CLEAN, SYSCALL_VMO_DESTROY, SYSCALL_VMO_SHARE_RO};

/// A device window no real device has (the top page below the synthetic ones of `vmo_runs`).
const SYNTHETIC_MMIO: usize = 0xffff_ffff_ffef_f000;

pub(super) fn run_vmo_clean_selftest(table: &SyscallTable, sys_ctx: &mut api::Context<'_>) {
    let desc = DeviceDesc {
        base: SYNTHETIC_MMIO as u64,
        len: PAGE_SIZE as u64,
        irq: 0,
        noncoherent: true,
        reach: DmaReach::ALL,
    };
    let dev = crate::mm::devices::register(desc).expect("synthetic non-coherent device");
    let kind = CapabilityKind::DeviceMmio {
        base: SYNTHETIC_MMIO,
        len: PAGE_SIZE,
        irq: 0,
        dma_noncoherent: true,
        dev,
    };
    let dev_slot = crate::cap::Held::plain(Capability { kind, rights: Rights::MAP })
        .and_then(|held| sys_ctx.tasks.current_caps_mut().allocate(held))
        .expect("synthetic device cap");
    let create = |table: &SyscallTable, ctx: &mut api::Context<'_>, device: usize| {
        let args = [usize::MAX, 2 * PAGE_SIZE, api::VMO_CREATE_LAZY, device, 0, 0];
        table.dispatch(SYSCALL_VMO_CREATE, ctx, &Args::new(args)).expect("vmo_create (lazy)")
    };
    let slot = create(table, sys_ctx, dev_slot);
    let id = match sys_ctx.tasks.current_caps_mut().get(slot).expect("vmo cap").kind {
        CapabilityKind::Vmo { id, .. } => id,
        _ => panic!("unexpected cap kind"),
    };
    let block = crate::hal::platform::cbom_block();
    let per_page = if block == 0 { 0 } else { PAGE_SIZE / block };

    // 1. The kernel commits page 0: its zeroes are cleaned before the page is entered.
    let before = lines_cleaned();
    let committed = crate::mm::vmo::commit(id, 0).is_ok();
    let zeroed = lines_cleaned() - before;
    // 2. vmo_clean over both pages: the committed one is cleaned, the absent one is nothing.
    let before = lines_cleaned();
    let clean = |s: usize, off: usize, len: usize| Args::new([s, off, len, 0, 0, 0]);
    let whole = table.dispatch(SYSCALL_VMO_CLEAN, sys_ctx, &clean(slot, 0, 2 * PAGE_SIZE));
    let lines = lines_cleaned() - before;
    // 3. Refused with their identity: a read-only alias, a range past the end.
    let alias = table
        .dispatch(SYSCALL_VMO_SHARE_RO, sys_ctx, &Args::new([slot, 0, 0, 0, 0, 0]))
        .expect("read-only alias");
    let deny_ro = matches!(
        table.dispatch(SYSCALL_VMO_CLEAN, sys_ctx, &clean(alias, 0, PAGE_SIZE)),
        Err(SysError::Capability(CapError::PermissionDenied))
    );
    let deny_end = matches!(
        table.dispatch(SYSCALL_VMO_CLEAN, sys_ctx, &clean(slot, PAGE_SIZE, 2 * PAGE_SIZE)),
        Err(SysError::AddressSpace(AddressSpaceError::InvalidArgs))
    );
    // 4. An object for no device: nothing to clean, success.
    let plain = create(table, sys_ctx, api::VMO_CREATE_NO_DEVICE);
    let before = lines_cleaned();
    let coherent_ok =
        table.dispatch(SYSCALL_VMO_CLEAN, sys_ctx, &clean(plain, 0, PAGE_SIZE)) == Ok(0);
    let coherent = lines_cleaned() - before;

    let _ = sys_ctx.tasks.current_caps_mut().take(alias);
    for s in [slot, plain] {
        let _ = table.dispatch(SYSCALL_VMO_DESTROY, sys_ctx, &Args::new([s, 0, 0, 0, 0, 0]));
    }
    let _ = sys_ctx.tasks.current_caps_mut().take(dev_slot);
    let retired = crate::mm::devices::retire(dev).is_ok();

    let deny = usize::from(deny_ro) + usize::from(deny_end);
    let counted = zeroed == per_page && lines == per_page && coherent == 0;
    if committed && whole == Ok(0) && deny == 2 && coherent_ok && counted && retired {
        log_info!(
            target: "selftest",
            "KSELFTEST: vmo clean ok (deny=2 block={} lines={} zeroed={} coherent=0)",
            block,
            lines,
            zeroed
        );
    } else {
        log_info!(
            target: "selftest",
            "KSELFTEST: vmo clean FAIL (committed={} whole={:?} deny={} block={} lines={} zeroed={} coherent={} retired={})",
            committed,
            whole,
            deny,
            block,
            lines,
            zeroed,
            coherent,
            retired
        );
    }
}
