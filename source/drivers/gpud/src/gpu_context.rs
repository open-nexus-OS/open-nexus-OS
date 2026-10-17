// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: a client's GPU context over IPC (TASK-0215 5c-3 P3, RFC-0101 §1, §7, §7b) — gpud's
//! half of `nexus_display_proto::gpu_context`. The capabilities are answered from the GPU the
//! display has. An open is served for windowd alone: identity is the kernel's
//! `sender_service_id`, never a payload field, and anyone else is refused by name. The
//! library's VMO is checked before it is mapped (a VMO whose window holds the declared bytes;
//! the wire bounds the declaration), mapped read-only for the take, unmapped, and its slot
//! closed whatever the answer — a moved capability has no Drop. The GPU that takes it is the
//! board's device beside its display (`backend::gpu::client_context`) or the virtio GPU's virgl
//! context on QEMU (`virgl_pipelines`); a client that holds a context has it replaced whole.
//! OWNERS: @gpu @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: unit tests below (the identity and VMO gates); the board's `gpud: gpu library
//!   ok (client=windowd`, the QEMU display lanes' `windowd: gpu context ok (`

#![cfg(any(test, all(feature = "os-lite", target_os = "none")))]

use nexus_display_proto::gpu_context::{ContextOpened, GpuCapabilities};
use nexus_display_proto::STATUS_UNSUPPORTED;

/// The GPU a display's clients open their contexts on.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) trait GpuService {
    /// What the device runs (`OP_GPU_CAPABILITIES`).
    fn capabilities(&self) -> GpuCapabilities;
    /// Opens `client`'s context at `priority` with `library` (`OP_GPU_CONTEXT_OPEN`); a context
    /// the client holds is replaced whole first. The sender was checked, the library mapped.
    fn open(&mut self, client: u64, priority: u8, library: &[u8]) -> ContextOpened;
    /// The device's interrupt fired: its messages served (the board's firmware).
    fn on_wake(&mut self) {}
}

/// What a display without a GPU answers.
pub(crate) const NO_GPU: GpuCapabilities =
    GpuCapabilities { status: STATUS_UNSUPPORTED, max_contexts: 0, features: 0, code_target: 0 };

/// An open refused with `status`.
pub(crate) const fn refused(status: u8) -> ContextOpened {
    ContextOpened { status, ctx: 0, accepted: 0, refused: 0, first_refused: 0 }
}

/// Who may open a context (deny by default): the display's compositor, windowd, by the
/// kernel's id of its name — an unnamed spawn (0) never.
pub(crate) fn may_open(sender: u64) -> bool {
    sender != 0 && sender == nexus_abi::service_id_from_name(b"windowd")
}

/// The library's VMO before it is mapped: a VMO (or its read-only alias) whose window holds
/// the `declared` bytes, at least one.
pub(crate) const fn library_vmo_fits(kind_tag: u32, window: u64, declared: u32) -> bool {
    matches!(kind_tag, 1 | 3) && declared != 0 && window >= declared as u64
}

/// `OP_GPU_CAPABILITIES` and `OP_GPU_CONTEXT_OPEN` in the request loop: the answer sent here,
/// sixteen bytes; a moved capability closed on every path.
#[cfg(all(feature = "os-lite", target_os = "none"))]
pub(crate) fn serve(
    server: &nexus_ipc::KernelServer,
    sender: u64,
    frame: &[u8],
    moved: Option<nexus_ipc::ReplyCap>,
    service: Option<&mut dyn GpuService>,
) {
    use nexus_display_proto::gpu_context as wire;
    use nexus_ipc::{Server as _, Wait};
    let reply = if wire::decode_capabilities_request(frame) {
        if let Some(cap) = moved {
            cap.close();
        }
        wire::encode_capabilities_reply(&service.map_or(NO_GPU, |s| s.capabilities()))
    } else {
        let answer = match moved {
            Some(cap) => {
                let answer = open(sender, frame, cap.slot(), service);
                cap.close();
                answer
            }
            None => refused(nexus_display_proto::STATUS_MALFORMED),
        };
        wire::encode_open_reply(&answer)
    };
    let _ = server.send(&reply, Wait::Blocking);
}

/// An open: the sender first, then the frame, the device, the VMO — the library mapped for the
/// take alone.
#[cfg(all(feature = "os-lite", target_os = "none"))]
fn open(
    sender: u64,
    frame: &[u8],
    vmo: u32,
    service: Option<&mut dyn GpuService>,
) -> ContextOpened {
    use nexus_display_proto::{STATUS_DENIED, STATUS_MALFORMED, STATUS_REFUSED};
    if !may_open(sender) {
        let _ = nexus_abi::debug_println("gpud: gpu context denied (the sender is not windowd)");
        return refused(STATUS_DENIED);
    }
    let Some(req) = nexus_display_proto::gpu_context::decode_open(frame) else {
        return refused(STATUS_MALFORMED);
    };
    let Some(service) = service else {
        return refused(STATUS_UNSUPPORTED);
    };
    let mut info = nexus_abi::CapQuery::default();
    if nexus_abi::cap_query(vmo, &mut info).is_err()
        || !library_vmo_fits(info.kind_tag, info.len, req.library_len)
    {
        return refused(STATUS_REFUSED);
    }
    with_library(vmo, req.library_len as usize, |bytes| service.open(sender, req.priority, bytes))
        .unwrap_or(refused(STATUS_REFUSED))
}

/// Maps the client's library read-only for one take, runs `f` on its bytes, unmaps it.
#[cfg(all(feature = "os-lite", target_os = "none"))]
fn with_library<R>(vmo: u32, len: usize, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
    let map_len = len.checked_add(4095)? & !4095;
    let flags =
        nexus_abi::page_flags::VALID | nexus_abi::page_flags::USER | nexus_abi::page_flags::READ;
    let va = nexus_abi::vm_map(vmo, 0, map_len, flags).ok()?;
    // SAFETY: `va..va + map_len` is the client's VMO, mapped read-only for this call alone and
    // checked to hold `len` bytes; the slice is read only inside `f` and dropped before the
    // unmap below. The bytes are the client's to change — the parse is bounded either way, and
    // what it places is GPU code in the client's own context.
    let bytes = unsafe { core::slice::from_raw_parts(va as *const u8, len) };
    let out = f(bytes);
    if let Err(err) = nexus_abi::vm_unmap(va, map_len) {
        crate::diag::err_line(b"library unmap", err);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windowd_opens_and_its_library_vmo_fits() {
        assert!(may_open(nexus_abi::service_id_from_name(b"windowd")));
        assert!(library_vmo_fits(1, 8192, 4748), "a VMO holding the bytes");
        assert!(library_vmo_fits(3, 4748, 4748), "its read-only alias, exactly");
    }

    #[test]
    fn test_reject_open_from_another_service() {
        for name in [&b"gpud"[..], b"screencapd", b"windowd2", b"Windowd", b""] {
            assert!(!may_open(nexus_abi::service_id_from_name(name)), "{name:?}");
        }
        assert!(!may_open(0), "an unnamed spawn");
    }

    #[test]
    fn test_reject_library_vmo_shorter_than_declared_or_of_another_kind() {
        assert!(!library_vmo_fits(1, 4747, 4748), "a byte short");
        assert!(!library_vmo_fits(2, 1 << 20, 4748), "a device window is no library");
        assert!(!library_vmo_fits(0, 1 << 20, 4748), "no capability kind");
        assert!(!library_vmo_fits(1, 4096, 0), "nothing declared");
    }

    #[test]
    fn a_display_without_a_gpu_answers_unsupported() {
        assert_eq!(NO_GPU.status, STATUS_UNSUPPORTED);
        assert_eq!(NO_GPU.code_target, 0);
        assert_eq!(refused(STATUS_UNSUPPORTED).ctx, 0);
    }
}
