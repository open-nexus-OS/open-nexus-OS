// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the `vmo_runs` door on the real kernel (RFC-0098 C4, TASK-0286 P4a):
//! `KSELFTEST: vmo runs ok (runs=… deny=…)`. The authority rule and the clipping
//! are host-tested in `crate::dma_runs`; this proves the wiring — the capability
//! resolution, the device check against the real cap table and the object's real
//! runs — with an anonymous object of two superpages and a page. Split out of
//! `selftest/mod.rs` (structure gate); runs inside `run_address_space_selftests`.
//! OWNERS: @kernel-mm-team
//! STATUS: Functional
//! TEST_COVERAGE: QEMU marker ladder (required in every profile)

use super::*;

/// Drive `resolve_runs` through its four outcomes on the bootstrap task.
pub(super) fn run_vmo_runs_selftests(table: &SyscallTable, sys_ctx: &mut api::Context<'_>) {
    use crate::syscall::{SYSCALL_VMO_DESTROY, SYSCALL_VMO_SHARE_RO};
    use crate::va_space::SUPERPAGE;
    let len = 2 * SUPERPAGE + PAGE_SIZE;
    let slot = table
        .dispatch(SYSCALL_VMO_CREATE, sys_ctx, &Args::new([usize::MAX, len, 0, 0, 0, 0]))
        .expect("vmo_create (anon) for vmo_runs");
    let id = match sys_ctx.tasks.current_caps_mut().get(slot).expect("runs vmo cap").kind {
        CapabilityKind::Vmo { id, .. } => id,
        _ => panic!("unexpected cap kind"),
    };
    let denied = |r: Result<usize, SysError>| {
        matches!(
            r,
            Err(SysError::Capability(crate::cap::CapError::PermissionDenied))
                | Err(SysError::AddressSpace(AddressSpaceError::InvalidArgs))
        )
    };
    let mut out = [(0u64, 0u64); 8];

    // 1. No device capability, no address.
    let had_device = sys_ctx.tasks.current_caps_mut().holds_device();
    let deny_device = !had_device && denied(api::resolve_runs(sys_ctx, slot, 0, len, &mut out));

    // 2. With one, the runs cover the object and name its frames.
    let (dev_base, dev_len) = crate::hal::platform::uart_window().expect("console window");
    let device = Capability {
        kind: CapabilityKind::DeviceMmio { base: dev_base, len: dev_len, irq: 0 },
        rights: Rights::MAP,
    };
    let dev_slot = sys_ctx.tasks.current_caps_mut().allocate(device).expect("device cap");
    let whole = api::resolve_runs(sys_ctx, slot, 0, len, &mut out);
    let runs = whole.unwrap_or(0);
    let covers = whole.is_ok()
        && out[..runs].iter().map(|r| r.1 as usize).sum::<usize>() == len
        && out[..runs].iter().all(|r| r.0 as usize % PAGE_SIZE == 0)
        && crate::mm::vmo::translate(id, 0) == Some(out[0].0 as usize);
    let inner = api::resolve_runs(sys_ctx, slot, SUPERPAGE + 17, 100, &mut out);
    let names_frames =
        inner == Ok(1) && crate::mm::vmo::translate(id, SUPERPAGE + 17) == Some(out[0].0 as usize);

    // 3. A read-only alias never yields one; 4. nor does a range past the end.
    let ro_slot = table
        .dispatch(SYSCALL_VMO_SHARE_RO, sys_ctx, &Args::new([slot, 0, 0, 0, 0, 0]))
        .expect("vmo_share_ro for vmo_runs");
    let deny_ro = denied(api::resolve_runs(sys_ctx, ro_slot, 0, PAGE_SIZE, &mut out));
    let deny_end =
        denied(api::resolve_runs(sys_ctx, slot, len - PAGE_SIZE, 2 * PAGE_SIZE, &mut out));

    let caps = sys_ctx.tasks.current_caps_mut();
    let _ = caps.take(dev_slot);
    let _ = caps.take(ro_slot);
    let _ = table.dispatch(SYSCALL_VMO_DESTROY, sys_ctx, &Args::new([slot, 0, 0, 0, 0, 0]));

    let deny = [deny_device, deny_ro, deny_end].iter().filter(|d| **d).count();
    if covers && names_frames && deny == 3 {
        log_info!(target: "selftest", "KSELFTEST: vmo runs ok (runs={} deny={})", runs, deny);
    } else {
        log_error!(
            target: "selftest",
            "KSELFTEST: vmo runs FAIL covers={} frames={} had_device={} deny_device={} deny_ro={} deny_end={}",
            covers,
            names_frames,
            had_device,
            deny_device,
            deny_ro,
            deny_end
        );
    }
}
