// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Runtime boot-configuration reader sourced from the device tree
//! (RFC-0098 C2/C3, TASK-0245 P4). The launcher's knobs (`selftest-mode`,
//! `selftest-profile`) reach the guest as `/chosen/nexus,boot-mode` and
//! `nexus,boot-profile`, written by nxboot; init pins the kernel's read-only
//! alias of the tree into the harness's declared slot (`NamedSlot::DeviceTree`)
//! and this module maps it once and reads the two properties. No MMIO, no
//! address: a tree without the knobs is an honest `None`.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: Unit tests for mode/profile token parsing (`runtime_mode.rs`);
//!   every QEMU lane resolves its profile through this path
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md

use core::sync::atomic::{AtomicUsize, Ordering};

use nexus_abi::yield_;

use crate::runtime_mode::{parse_runtime_mode, parse_runtime_profile, RuntimeMode, RuntimeProfile};

/// Where the tree is mapped (0 = not yet), and its `totalsize`.
static TREE_VA: AtomicUsize = AtomicUsize::new(0);
static TREE_LEN: AtomicUsize = AtomicUsize::new(0);

const RUNTIME_CFG_RETRY_YIELDS: usize = 8_192;

/// Master gate for the interactive display-bootstrap OBSERVER/driver path (`end.rs`:
/// `observe_display_evidence` / `display_bootstrap::run` / `interactive_live_tick`, which POLL
/// windowd with `GET_VISIBLE_STATE` up to 128×). This path was implicitly OFF in EVERY boot to
/// date because `runtime_mode` could never resolve — the boot-config channel was never granted.
/// Granting it flipped `runtime_mode` to `Some`, which would have activated this
/// never-before-run driver as a side effect of enabling verdict mode. It is NOT a pure
/// observer (it drives windowd), so until the pure-observer refactor (task #98) it stays
/// gated OFF — the verdict console keys off `runtime_is_interactive`, NOT this, so verdicts work
/// without dragging in the untested windowd-polling path. Flip to `true` to re-enable it.
const INTERACTIVE_DISPLAY_OBSERVER_ENABLED: bool = false;

#[must_use]
pub(crate) fn display_bootstrap_enabled() -> bool {
    INTERACTIVE_DISPLAY_OBSERVER_ENABLED && runtime_mode_with_retry().is_some()
}

#[must_use]
pub(crate) fn runtime_mode() -> Option<RuntimeMode> {
    parse_runtime_mode(chosen_str("boot-mode")?.as_bytes())
}

#[must_use]
pub(crate) fn runtime_mode_with_retry() -> Option<RuntimeMode> {
    retry_runtime_config(runtime_mode)
}

#[must_use]
pub(crate) fn runtime_profile() -> Option<RuntimeProfile> {
    parse_runtime_profile(chosen_str("boot-profile")?.as_bytes())
}

#[must_use]
pub(crate) fn runtime_profile_with_retry() -> Option<RuntimeProfile> {
    retry_runtime_config(runtime_profile)
}

/// What the tree's root names (TASK-0327B P4 H0b): QEMU's `virt` machine is `riscv-virtio`;
/// anything else that reads is a board. RFC-0098 C2 makes an absent `nexus,boot-mode` a valid
/// proof boot on QEMU (raw markers), so "no knobs in `/chosen`" can never mean "board" by
/// itself — the machine does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Machine {
    /// QEMU `virt` (`compatible = "riscv-virtio"`): the lanes' machine.
    QemuVirt,
    /// A tree whose root is not QEMU's: real hardware.
    Board,
    /// No readable tree (an image without the pinned alias): decided by the knobs alone.
    Unknown,
}

#[must_use]
pub(crate) fn machine() -> Machine {
    let Some(bytes) = tree_bytes() else {
        return Machine::Unknown;
    };
    let Ok(fdt) = nexus_fdt::Fdt::new(bytes) else {
        return Machine::Unknown;
    };
    let Ok(root) = fdt.root() else {
        return Machine::Unknown;
    };
    if root.is_compatible("riscv-virtio") {
        Machine::QemuVirt
    } else {
        Machine::Board
    }
}

/// `/chosen/nexus,<name>` from the tree init pinned for us.
fn chosen_str(name: &str) -> Option<&'static str> {
    let fdt = nexus_fdt::Fdt::new(tree_bytes()?).ok()?;
    fdt.chosen().ok()?.nexus_str(name)
}

/// The tree as bytes, mapped read-only on first use from the declared slot.
fn tree_bytes() -> Option<&'static [u8]> {
    let va = TREE_VA.load(Ordering::Acquire);
    if va != 0 {
        let len = TREE_LEN.load(Ordering::Acquire);
        // SAFETY: the slice `nexus_abi::device_tree::map_read_only` returned, remembered.
        return Some(unsafe { core::slice::from_raw_parts(va as *const u8, len) });
    }
    let slot = nexus_service_topology::slots::selftest_client::DEVICE_TREE;
    let bytes = nexus_abi::device_tree::map_read_only(slot)?;
    TREE_LEN.store(bytes.len(), Ordering::Release);
    TREE_VA.store(bytes.as_ptr() as usize, Ordering::Release);
    Some(bytes)
}

fn retry_runtime_config<T>(mut read: impl FnMut() -> Option<T>) -> Option<T> {
    for _ in 0..RUNTIME_CFG_RETRY_YIELDS {
        if let Some(value) = read() {
            return Some(value);
        }
        let _ = yield_();
    }
    read()
}
