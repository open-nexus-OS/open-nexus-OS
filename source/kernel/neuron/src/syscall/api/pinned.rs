// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: A memory object's capability held for one call (RFC-0102, TASK-0330 P3/P4). A
//! syscall that names a memory object by a slot works on the object's id after it left the
//! capability table's lock; what keeps the object — and with it the meaning of that id —
//! alive for the call is a second, counted capability the call holds itself: a sibling
//! thread closing or moving the slot meanwhile frees nothing under it, and the judge of an
//! object nobody holds (`mm::reclaim`), which runs beside any syscall, never takes the
//! object. Dropped, the pin counts itself off and, was it the last capability, judges the
//! object's life there.
//! OWNERS: @kernel-team
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU `KSELFTEST: vm map reject ok`, `KSELFTEST: vm unmap reclaim ok`,
//!   `KSELFTEST: exec image reject ok`, the `smp8` rank lane; the counts on the host
//!   (`cap/tests_prop.rs`)
//! INVARIANTS: only a memory object is held — any other kind is refused by the caller's own
//!   check of `cap`, and counting it here would hide its last close from its judge; a pin is
//!   dropped with no address space held (its judge frees frames and may tell the pager).

use super::*;

/// The capability of one slot, checked for a right, and — a memory object's — held until
/// this value is dropped.
pub(super) struct Pinned {
    pub cap: Capability,
    held: Option<crate::cap::Held>,
    by: &'static str,
}

/// The capability in `slot` of `caps` with `rights`, pinned; `by` names the caller on the
/// console should the pin turn out to be the object's last capability.
pub(super) fn pin(
    caps: &crate::cap::CapTable,
    slot: usize,
    rights: Rights,
    by: &'static str,
) -> Result<Pinned, CapError> {
    let cap = caps.derive(slot, rights)?;
    let held = match cap.kind {
        CapabilityKind::Vmo { .. } | CapabilityKind::VmoRo { .. } => Some(caps.dup(slot, rights)?),
        _ => None,
    };
    Ok(Pinned { cap, held, by })
}

impl Pinned {
    /// The memory object held, with its length and whether the capability is the read-only
    /// kind; `None` for any other kind (nothing is held then).
    pub(super) fn object(&self) -> Option<(u32, usize, bool)> {
        match (&self.held, self.cap.kind) {
            (Some(_), CapabilityKind::Vmo { id, len }) => Some((id, len, false)),
            (Some(_), CapabilityKind::VmoRo { id, len }) => Some((id, len, true)),
            _ => None,
        }
    }
}

impl Drop for Pinned {
    fn drop(&mut self) {
        let Some((id, ..)) = self.object() else { return };
        // Counted off first: the judge reads the object's own counts.
        self.held = None;
        crate::mm::reclaim::judge(id, self.by);
    }
}
