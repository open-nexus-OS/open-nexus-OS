// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the memory level at the GPU driver (TASK-0333 6e-3, RFC-0100 §3, ADR-0086). memd —
//! the one memory authority — pushes `nexus_wire::memd::OP_LEVEL` to gpud's server endpoint as a
//! `'M','D'` frame (no gpud op byte is `'M'`); gpud takes it from memd's kernel sender id alone
//! (`may_tell`, host-tested) and names what it heard: `gpud: mem pressure (level= source=)`.
//! What gpud DROPS at `warn` is 6e-4's, with the per-client ledger that says what a cache costs
//! whom (on the board the glass cache is a GPU-private buffer of the one pool; on QEMU the
//! host's own GPU memory) — here the level arrives on the path that work rides. A push: never
//! answered, never a reply on the shared response endpoint windowd reads.
//! OWNERS: @gpu
//! STATUS: Functional (6e-3: the level heard; the drop is 6e-4)
//! API_STABILITY: Internal
//! TEST_COVERAGE: `test_reject_level_from_anyone_but_memd` (host); the visible lane's
//!   `gpud: mem pressure (level=warn` after the harness's drill

/// Whether `sender` (a kernel service id) may tell gpud the memory level: memd alone.
#[must_use]
pub(crate) fn may_tell(sender: u64) -> bool {
    sender != 0 && sender == nexus_abi::service_id_from_name(b"memd")
}

/// `true` when `frame` was the authority's level push (taken or refused — either way handled,
/// no reply); `false` for any other frame.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) fn on_frame(frame: &[u8], sender: u64) -> bool {
    let Some((level, source, _free, _total)) = nexus_wire::memd::decode_level(frame) else {
        return false;
    };
    if !may_tell(sender) {
        let _ = nexus_abi::debug_println("gpud: mem pressure denied (sender)");
        return true;
    }
    let _ = nexus_abi::debug_println(&alloc::format!(
        "gpud: mem pressure (level={} source={})",
        nexus_abi::syscall::memory::mem_pressure_name(level),
        nexus_wire::memd::source_name(source)
    ));
    true
}

#[cfg(test)]
mod tests {
    use super::may_tell;

    #[test]
    fn test_reject_level_from_anyone_but_memd() {
        assert!(may_tell(nexus_abi::service_id_from_name(b"memd")));
        for name in [&b"windowd"[..], b"abilitymgr", b"selftest-client", b"app:calculator"] {
            assert!(!may_tell(nexus_abi::service_id_from_name(name)), "{name:?}");
        }
        assert!(!may_tell(0), "nobody");
    }
}
