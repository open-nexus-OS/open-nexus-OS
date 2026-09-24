// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the DMA door on the real kernel (RFC-0098 C4, TASK-0286 P4a, device-scoped
//! by TASK-0246 P1). `KSELFTEST: vmo runs ok (runs=… deny=…)` proves the `vmo_runs`
//! wiring — the two capabilities, the device record and the object's real runs — with
//! an anonymous object of two superpages and a page and a device that reaches
//! everything. `KSELFTEST: vmo reach ok (…)` proves the reach: a synthetic device
//! whose one translated window is the top of the first memory bank gets its objects
//! from inside that window and their runs in its bus addresses; an object made for
//! another device (the window just below) has no bus address for it, and a contiguous
//! object for no device is refused. The rules themselves are host-tested (`crate::dma_runs`,
//! `crate::dma_reach`, `crate::frames`). Split out of `selftest/mod.rs` (structure
//! gate); runs inside `run_address_space_selftests`.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! TEST_COVERAGE: QEMU marker ladder (required in every profile)

use super::*;
use crate::dma_reach::{DeviceDesc, DeviceId, DmaReach, DmaWindow};
use crate::syscall::SYSCALL_VMO_DESTROY;

/// Register windows no real device has (the top of the physical address space): a
/// synthetic record lives only while a selftest runs and is retired before init.
const SYNTHETIC_MMIO: usize = 0xffff_ffff_fff0_0000;

/// A device capability over a synthetic record reaching `reach`, in a fresh slot of
/// the running task: `(slot, dev)`. `index` keeps the register windows apart.
pub(super) fn synthetic_device(
    sys_ctx: &mut api::Context<'_>,
    index: usize,
    reach: DmaReach,
) -> (usize, DeviceId) {
    let base = SYNTHETIC_MMIO + index * PAGE_SIZE;
    let desc =
        DeviceDesc { base: base as u64, len: PAGE_SIZE as u64, irq: 0, noncoherent: false, reach };
    let dev = crate::mm::devices::register(desc).expect("synthetic device record");
    let kind =
        CapabilityKind::DeviceMmio { base, len: PAGE_SIZE, irq: 0, dma_noncoherent: false, dev };
    let cap = Capability { kind, rights: Rights::MAP };
    let slot = sys_ctx.tasks.current_caps_mut().allocate(cap).expect("synthetic device cap");
    (slot, dev)
}

/// Drop the capability and retire the record; true when the record is gone.
pub(super) fn drop_synthetic_device(
    sys_ctx: &mut api::Context<'_>,
    slot: usize,
    dev: DeviceId,
) -> bool {
    let _ = sys_ctx.tasks.current_caps_mut().take(slot);
    crate::mm::devices::retire(dev).is_ok() && crate::mm::devices::reach(dev).is_none()
}

fn vmo_id(sys_ctx: &mut api::Context<'_>, slot: usize) -> u32 {
    match sys_ctx.tasks.current_caps_mut().get(slot).expect("vmo cap").kind {
        CapabilityKind::Vmo { id, .. } => id,
        _ => panic!("unexpected cap kind"),
    }
}

fn create(
    table: &SyscallTable,
    sys_ctx: &mut api::Context<'_>,
    len: usize,
    flags: usize,
    device: usize,
) -> Result<usize, SysError> {
    table.dispatch(SYSCALL_VMO_CREATE, sys_ctx, &Args::new([usize::MAX, len, flags, device, 0, 0]))
}

fn destroy(table: &SyscallTable, sys_ctx: &mut api::Context<'_>, slot: usize) {
    let _ = table.dispatch(SYSCALL_VMO_DESTROY, sys_ctx, &Args::new([slot, 0, 0, 0, 0, 0]));
}

fn denied(r: Result<usize, SysError>) -> bool {
    matches!(
        r,
        Err(SysError::Capability(crate::cap::CapError::PermissionDenied))
            | Err(SysError::AddressSpace(AddressSpaceError::InvalidArgs))
    )
}

/// Drive `resolve_runs` through its outcomes on the bootstrap task.
pub(super) fn run_vmo_runs_selftests(table: &SyscallTable, sys_ctx: &mut api::Context<'_>) {
    use crate::syscall::SYSCALL_VMO_SHARE_RO;
    use crate::va_space::SUPERPAGE;
    let len = 2 * SUPERPAGE + PAGE_SIZE;
    let slot = create(table, sys_ctx, len, 0, api::VMO_CREATE_NO_DEVICE)
        .expect("vmo_create (anon) for vmo_runs");
    let id = vmo_id(sys_ctx, slot);
    let mut out = [(0u64, 0u64); 8];

    // 1. The device slot names no slot, 2. or holds a VMO: no device, no address.
    let deny_no_slot = denied(api::resolve_runs(sys_ctx, slot, usize::MAX, 0, len, &mut out));
    let deny_not_device = denied(api::resolve_runs(sys_ctx, slot, slot, 0, len, &mut out));

    // 3. With a device that reaches everything, the runs cover the object and name
    //    its frames (identity).
    let (dev_slot, dev) = synthetic_device(sys_ctx, 1, DmaReach::ALL);
    let whole = api::resolve_runs(sys_ctx, slot, dev_slot, 0, len, &mut out);
    let runs = whole.unwrap_or(0);
    let covers = whole.is_ok()
        && out[..runs].iter().map(|r| r.1 as usize).sum::<usize>() == len
        && out[..runs].iter().all(|r| r.0 as usize % PAGE_SIZE == 0)
        && crate::mm::vmo::translate(id, 0) == Some(out[0].0 as usize);
    let inner = api::resolve_runs(sys_ctx, slot, dev_slot, SUPERPAGE + 17, 100, &mut out);
    let names_frames =
        inner == Ok(1) && crate::mm::vmo::translate(id, SUPERPAGE + 17) == Some(out[0].0 as usize);

    // 4. A read-only alias never yields one; 5. nor does a range past the end.
    let ro_slot = table
        .dispatch(SYSCALL_VMO_SHARE_RO, sys_ctx, &Args::new([slot, 0, 0, 0, 0, 0]))
        .expect("vmo_share_ro for vmo_runs");
    let deny_ro = denied(api::resolve_runs(sys_ctx, ro_slot, dev_slot, 0, PAGE_SIZE, &mut out));
    let deny_end = denied(api::resolve_runs(
        sys_ctx,
        slot,
        dev_slot,
        len - PAGE_SIZE,
        2 * PAGE_SIZE,
        &mut out,
    ));

    let _ = sys_ctx.tasks.current_caps_mut().take(ro_slot);
    let retired = drop_synthetic_device(sys_ctx, dev_slot, dev);
    destroy(table, sys_ctx, slot);

    let deny = [deny_no_slot, deny_not_device, deny_ro, deny_end].iter().filter(|d| **d).count();
    if covers && names_frames && deny == 4 && retired {
        log_info!(target: "selftest", "KSELFTEST: vmo runs ok (runs={} deny={})", runs, deny);
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: vmo runs FAIL covers={} frames={} deny_no_slot={} deny_not_device={} deny_ro={} deny_end={} retired={}",
            covers,
            names_frames,
            deny_no_slot,
            deny_not_device,
            deny_ro,
            deny_end,
            retired
        );
    }
}

/// The top `WINDOW` bytes of the first memory bank: memory the pool hands out last,
/// so an object found there was placed there.
const WINDOW: u64 = 64 << 20;
/// The window's device-visible base: a translated window, like the board's
/// multimedia and network buses.
const BUS: u64 = 0x40_0000_0000;

/// Allocation inside a device's reach and the answer in its bus addresses.
pub(super) fn run_vmo_reach_selftests(table: &SyscallTable, sys_ctx: &mut api::Context<'_>) {
    let bank = crate::boot_fdt::bytes()
        .and_then(|b| nexus_fdt::Fdt::new(b).ok())
        .and_then(|fdt| fdt.memory_banks().next());
    let Some(bank) = bank.filter(|b| b.size >= 2 * WINDOW) else {
        log_error!(target: "selftest", "KSELFTEST: vmo reach FAIL no memory bank of 2x{} bytes", WINDOW);
        return;
    };
    let cpu = bank.base + bank.size - WINDOW;
    let reach = DmaReach::new(&[DmaWindow { bus: BUS, cpu, size: WINDOW }]).expect("reach");
    let inside = |pa: usize, len: usize| cpu <= pa as u64 && pa as u64 + len as u64 <= cpu + WINDOW;
    let bus_of = |pa: usize| BUS + (pa as u64 - cpu);
    let (dev_slot, dev) = synthetic_device(sys_ctx, 2, reach);
    let mut out = [(0u64, 0u64); 8];

    // 1. An anonymous object for the device lies in the window; its runs come back
    //    translated, whole and at an inner offset.
    let anon_len = 3 * PAGE_SIZE;
    let anon = create(table, sys_ctx, anon_len, 0, dev_slot).expect("vmo_create for a device");
    let anon_id = vmo_id(sys_ctx, anon);
    let anon_placed = crate::mm::vmo::runs(anon_id)
        .is_some_and(|runs| runs.iter().all(|&(pa, len)| inside(pa, len)));
    let whole = api::resolve_runs(sys_ctx, anon, dev_slot, 0, anon_len, &mut out);
    let anon_runs = whole.unwrap_or(0);
    let anon_translated = whole.is_ok()
        && out[..anon_runs].iter().map(|r| r.1 as usize).sum::<usize>() == anon_len
        && crate::mm::vmo::translate(anon_id, 0).map(bus_of) == Some(out[0].0);
    let inner = api::resolve_runs(sys_ctx, anon, dev_slot, PAGE_SIZE + 17, 100, &mut out);
    let inner_translated = inner == Ok(1)
        && crate::mm::vmo::translate(anon_id, PAGE_SIZE + 17).map(bus_of) == Some(out[0].0);

    // 2. A contiguous object for the device: one run in the window, one bus base.
    let contig_len = 2 * PAGE_SIZE;
    let contig = create(table, sys_ctx, contig_len, api::VMO_CREATE_CONTIGUOUS, dev_slot)
        .expect("vmo_create (contiguous) for a device");
    let contig_pa = crate::mm::vmo::first_pa(vmo_id(sys_ctx, contig));
    let contig_placed = contig_pa.is_some_and(|pa| inside(pa, contig_len));
    let contig_translated = api::resolve_runs(sys_ctx, contig, dev_slot, 0, contig_len, &mut out)
        == Ok(1)
        && contig_pa.map(|pa| (bus_of(pa), contig_len as u64)) == Some(out[0]);

    // 3. A contiguous object for no device is refused: nobody could say where it
    //    must lie.
    let deny_contig = matches!(
        create(table, sys_ctx, PAGE_SIZE, api::VMO_CREATE_CONTIGUOUS, api::VMO_CREATE_NO_DEVICE),
        Err(SysError::AddressSpace(AddressSpaceError::InvalidArgs))
    );

    // 4. An object made for another device, whose one window (identity) is the one just
    //    below, lies outside this device's window by construction — not by the pool's
    //    order: this device has no bus address for it, and the answer is refused whole.
    let below = DmaWindow { bus: cpu - WINDOW, cpu: cpu - WINDOW, size: WINDOW };
    let (other_slot, other) =
        synthetic_device(sys_ctx, 3, DmaReach::new(&[below]).expect("reach below"));
    let foreign =
        create(table, sys_ctx, PAGE_SIZE, 0, other_slot).expect("vmo_create for another device");
    let foreign_outside = crate::mm::vmo::translate(vmo_id(sys_ctx, foreign), 0)
        .is_some_and(|pa| !inside(pa, PAGE_SIZE));
    let deny_reach = foreign_outside
        && denied(api::resolve_runs(sys_ctx, foreign, dev_slot, 0, PAGE_SIZE, &mut out));

    for slot in [anon, contig, foreign] {
        destroy(table, sys_ctx, slot);
    }
    let retired = drop_synthetic_device(sys_ctx, dev_slot, dev)
        & drop_synthetic_device(sys_ctx, other_slot, other);

    let placed = anon_placed && contig_placed;
    let translated = anon_translated && inner_translated && contig_translated;
    let deny = [deny_contig, deny_reach].iter().filter(|d| **d).count();
    if placed && translated && deny == 2 && retired {
        log_info!(
            target: "selftest",
            "KSELFTEST: vmo reach ok (window=0x{:x}+0x{:x} bus=0x{:x} runs={} deny={})",
            cpu,
            WINDOW,
            BUS,
            anon_runs,
            deny
        );
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: vmo reach FAIL anon_placed={} contig_placed={} anon_translated={} inner_translated={} contig_translated={} deny_contig={} foreign_outside={} deny_reach={} retired={}",
            anon_placed,
            contig_placed,
            anon_translated,
            inner_translated,
            contig_translated,
            deny_contig,
            foreign_outside,
            deny_reach,
            retired
        );
    }
}
