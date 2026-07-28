// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: RFC-0085 kernel-owned VA allocation selftests — the gated
//! `KSELFTEST: vm map ok / vm unmap ok / vm map reject ok` markers. Split
//! out of `selftest/mod.rs` (structure-gate); runs inside
//! `run_address_space_selftests` against the address space that test
//! creates. Target-gated like its caller.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional (RFC-0085 Phase 3)
//! TEST_COVERAGE: QEMU marker ladder (headless + full profiles gate these)
//! ADR: docs/adr/0054-map-errors-keep-their-identity-across-the-abi.md

use super::*;

// RFC-0085: kernel-owned VA allocation. The bootstrap task has no
// address space of its own, so this exercises the mm executor
// (`vm_ops`) the vm_map/vm_unmap syscalls are thin shells over,
// against the AS created above; the userspace syscall path is proven
// end-to-end by the selftest-client roundtrip probe.
pub(super) fn run_vm_alloc_selftests(
    table: &SyscallTable,
    sys_ctx: &mut api::Context<'_>,
    handle_raw: usize,
) {
    use crate::mm::vm_ops;
    use crate::mm::PageFlags;
    use crate::syscall::SYSCALL_VMO_DESTROY;
    use crate::va_space::{RegionKind, SUPERPAGE};
    const VM_VMO_SLOT: usize = 5;
    // Two superpages + one page: at least one whole 2 MiB interior
    // span for every pa phase, plus 4K head/tail coverage.
    let vm_len = 2 * SUPERPAGE + PAGE_SIZE;
    table
        .dispatch(SYSCALL_VMO_CREATE, sys_ctx, &Args::new([VM_VMO_SLOT, vm_len, 0, 0, 0, 0]))
        .expect("vmo_create for vm_map");
    let cap = sys_ctx.tasks.bootstrap_mut().caps_mut().get(VM_VMO_SLOT).expect("vm vmo cap");
    let vm_base = match cap.kind {
        CapabilityKind::Vmo { base, .. } => base,
        _ => panic!("unexpected cap kind"),
    };
    let handle = crate::mm::AsHandle::from_raw(handle_raw as u32).expect("as handle");
    let flags = PageFlags::VALID | PageFlags::USER | PageFlags::READ | PageFlags::WRITE;

    // vm map ok: va inside the window, translation matches the
    // backing pa, and the eligible interior really is a 2 MiB leaf
    // (promotion PROVEN, not assumed).
    let space = sys_ctx.address_spaces.get_mut(handle).expect("as space");
    let va = vm_ops::map_range(space, vm_base, vm_len, flags, RegionKind::Vmo).expect("vm map");
    let window_end = crate::mm::USER_VM_WINDOW_BASE + crate::mm::USER_VM_WINDOW_LEN;
    let in_window = va >= crate::mm::USER_VM_WINDOW_BASE && va + vm_len <= window_end;
    let translates = space.page_table().translate(va) == Some(vm_base)
        && space.page_table().translate(va + vm_len - PAGE_SIZE)
            == Some(vm_base + vm_len - PAGE_SIZE);
    let interior = (va + SUPERPAGE - 1) & !(SUPERPAGE - 1);
    let promoted = vm_ops::leaf_span_at(space, interior) == Some(SUPERPAGE);
    if in_window && translates && promoted {
        log_info!(target: "selftest", "KSELFTEST: vm map ok");
    } else {
        log_info!(
            target: "selftest",
            "KSELFTEST: vm map FAIL va=0x{:x} in_window={} translates={} promoted={}",
            va,
            in_window,
            translates,
            promoted
        );
    }

    // Destroy-while-mapped must refuse with EBUSY-class (checked
    // here while the mapping is live; reported with the reject
    // marker below).
    let destroy_busy = matches!(
        table.dispatch(SYSCALL_VMO_DESTROY, sys_ctx, &Args::new([VM_VMO_SLOT, 0, 0, 0, 0, 0])),
        Err(crate::syscall::Error::ResourceBusy)
    );

    // vm unmap ok: translation gone, and a re-map of the same length
    // returns the SAME va (first-fit reuse — the anti-bump-arena
    // proof at executor level).
    let space = sys_ctx.address_spaces.get_mut(handle).expect("as space");
    vm_ops::unmap_range(space, va, vm_len).expect("vm unmap");
    let gone = space.page_table().translate(va).is_none()
        && space.page_table().translate(interior).is_none();
    let va2 = vm_ops::map_range(space, vm_base, vm_len, flags, RegionKind::Vmo).expect("vm re-map");
    if gone && va2 == va {
        log_info!(target: "selftest", "KSELFTEST: vm unmap ok");
    } else {
        log_info!(
            target: "selftest",
            "KSELFTEST: vm unmap FAIL gone={} va=0x{:x} va2=0x{:x}",
            gone,
            va,
            va2
        );
    }

    // vm map reject ok: fixed-VA into the managed window refused
    // (Overlap→EEXIST), unmap of an unmapped window va refused
    // (NotFound→ENOENT), and the destroy-while-mapped EBUSY above.
    let fixed_refused = matches!(
        sys_ctx.address_spaces.map_page_tracked(
            handle,
            crate::mm::USER_VM_WINDOW_BASE + 0x100_0000,
            vm_base,
            flags
        ),
        Err(crate::mm::AddressSpaceError::Mapping(crate::mm::MapError::Overlap))
    );
    let space = sys_ctx.address_spaces.get_mut(handle).expect("as space");
    let unknown_refused = matches!(
        vm_ops::unmap_range(space, crate::mm::USER_VM_WINDOW_BASE + 0x200_0000, PAGE_SIZE),
        Err(vm_ops::VmOpError::Va(crate::va_space::VaError::NotFound))
    );
    if fixed_refused && unknown_refused && destroy_busy {
        log_info!(target: "selftest", "KSELFTEST: vm map reject ok");
    } else {
        log_info!(
            target: "selftest",
            "KSELFTEST: vm map reject FAIL fixed={} unknown={} busy={}",
            fixed_refused,
            unknown_refused,
            destroy_busy
        );
    }

    // Cleanup: release the re-map and the VMO (now destroyable —
    // also proves the EBUSY guard clears with the last region).
    vm_ops::unmap_range(space, va2, vm_len).expect("vm cleanup unmap");
    table
        .dispatch(SYSCALL_VMO_DESTROY, sys_ctx, &Args::new([VM_VMO_SLOT, 0, 0, 0, 0, 0]))
        .expect("vmo_destroy after unmap");
}

/// The W^X negative proof (`KSELFTEST: w^x enforced`): a WRITE+EXECUTE user
/// mapping must be refused with `MapError::PermissionDenied`. One SSOT for
/// the two call paths (direct-run and scheduler-fallback) that used to carry
/// verbatim copies.
pub(super) fn run_wx_selftest(ctx: &mut Context<'_>, handle_raw: usize) {
    let mut table = SyscallTable::new();
    api::install_handlers(&mut table);
    let timer = ctx.hal.timer();
    let mut sys_ctx = api::Context::new(
        ctx.scheduler,
        ctx.tasks,
        ctx.router,
        ctx.address_spaces,
        timer,
        ctx.hart_timers,
        ctx.waitsets,
        ctx.fences,
    );
    const PROT_READ: usize = 1 << 0;
    const PROT_WRITE: usize = 1 << 1;
    const PROT_EXEC: usize = 1 << 2;
    const MAP_FLAG_USER: usize = 1 << 0;
    let wx_args = Args::new([
        handle_raw,
        2,
        CHILD_TEST_VA + PAGE_SIZE,
        PAGE_SIZE,
        PROT_READ | PROT_WRITE | PROT_EXEC,
        MAP_FLAG_USER,
    ]);
    match table.dispatch(SYSCALL_AS_MAP, &mut sys_ctx, &wx_args) {
        Err(SysError::AddressSpace(AddressSpaceError::Mapping(MapError::PermissionDenied))) => {
            log_info!(target: "selftest", "KSELFTEST: w^x enforced");
        }
        Err(_) | Ok(_) => {
            log_error!(target: "selftest", "KSELFTEST: w^x NOT enforced");
        }
    }
}
