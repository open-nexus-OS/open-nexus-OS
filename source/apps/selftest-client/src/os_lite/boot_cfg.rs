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

const FDT_MAGIC: u32 = 0xd00d_feed;
const MAX_DTB_LEN: usize = 1024 * 1024;
const PAGE: usize = 4096;
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
        // SAFETY: mapped read-only below for at least `len` bytes; never unmapped.
        return Some(unsafe { core::slice::from_raw_parts(va as *const u8, len) });
    }
    let slot = nexus_service_topology::slots::selftest_client::DEVICE_TREE;
    let mut info = nexus_abi::CapQuery { kind_tag: 0, irq: 0, base: 0, len: 0 };
    nexus_abi::cap_query(slot, &mut info).ok()?;
    let pages = usize::try_from(info.len).ok()?;
    if pages == 0 || pages % PAGE != 0 || pages > MAX_DTB_LEN + PAGE {
        return None;
    }
    let flags =
        nexus_abi::page_flags::VALID | nexus_abi::page_flags::READ | nexus_abi::page_flags::USER;
    let va = nexus_abi::vm_map(slot, 0, pages, flags).ok()?;
    // SAFETY: `pages` bytes are mapped at `va`, read-only; the header bounds the tree.
    let head = unsafe { core::slice::from_raw_parts(va as *const u8, 8) };
    let magic = u32::from_be_bytes([head[0], head[1], head[2], head[3]]);
    let total = u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as usize;
    if magic != FDT_MAGIC || total < 40 || total > pages {
        return None;
    }
    TREE_LEN.store(total, Ordering::Release);
    TREE_VA.store(va, Ordering::Release);
    Some(unsafe { core::slice::from_raw_parts(va as *const u8, total) })
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
