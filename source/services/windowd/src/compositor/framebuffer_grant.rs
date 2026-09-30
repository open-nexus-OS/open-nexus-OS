// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd's half of the framebuffer grant (RFC-0093 §5 as amended 2026-09-30, RFC-0098
//! C7, TASK-0251 P1). windowd no longer makes its framebuffer and reads no mode source: at start
//! it asks gpud once — the display-mode authority and the owner of the scanout memory — and
//! builds its compositor at the granted mode on the granted object. The wait has no clock: the
//! answer or gpud's death ends it (`nexus_ipc::exchange`). A display stack without gpud (no
//! display device — the board before its controller's driver) is NAMED, not faked: windowd runs
//! display-less at the layout maximum, composes into nothing and attaches nothing.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the wire in `nexus_display_proto::grant` (host, `test_reject_*`); every QEMU
//!   display lane (`windowd: display mode from gpud (WxH)`), the board (`windowd: display none`)

use nexus_abi::Handle;

/// What windowd starts from: the visible mode and, when gpud granted one, the framebuffer.
pub(crate) struct Grant {
    pub(crate) mode: (u32, u32),
    pub(crate) framebuffer: Option<Handle>,
}

/// Asks gpud for the framebuffer and the mode (once, at start).
#[cfg(nexus_env = "os")]
pub(crate) fn request() -> Grant {
    use nexus_display_proto::{
        decode_framebuffer_grant, encode_framebuffer_request, FRAMEBUFFER_GRANT_LEN, STATUS_OK,
    };
    let route = nexus_service_topology::slots::windowd::GPUD;
    if let Err(err) = nexus_ipc::exchange::send_request(route.send, &encode_framebuffer_request()) {
        return display_none(&alloc::format!("gpud unreachable: {err:?}"));
    }
    let mut frame = [0u8; FRAMEBUFFER_GRANT_LEN];
    let (len, moved) = match nexus_ipc::exchange::recv_response_with_cap(route.recv, &mut frame) {
        Ok(answer) => answer,
        Err(err) => return display_none(&alloc::format!("gpud gone: {err:?}")),
    };
    match (decode_framebuffer_grant(&frame[..len.min(frame.len())]), moved) {
        (Some(grant), Some(framebuffer)) if grant.status == STATUS_OK => {
            let mode = (u32::from(grant.mode_w), u32::from(grant.mode_h));
            let _ = nexus_abi::debug_println(&alloc::format!(
                "windowd: display mode from gpud ({}x{})",
                mode.0,
                mode.1
            ));
            Grant { mode, framebuffer: Some(framebuffer) }
        }
        (grant, moved) => {
            if let Some(slot) = moved {
                let _ = nexus_abi::cap_close(slot);
            }
            let status = grant.map(|g| g.status);
            display_none(&alloc::format!("grant refused: status={status:?}"))
        }
    }
}

/// Host builds have no gpud: the layout maximum, no framebuffer.
#[cfg(not(nexus_env = "os"))]
pub(crate) fn request() -> Grant {
    Grant { mode: nexus_display_proto::LAYOUT_MAX, framebuffer: None }
}

/// No display: named once, the layout maximum as the compositor's space, no framebuffer.
#[cfg(nexus_env = "os")]
fn display_none(why: &str) -> Grant {
    let _ = nexus_abi::debug_println(&alloc::format!("windowd: display none ({why})"));
    Grant { mode: nexus_display_proto::LAYOUT_MAX, framebuffer: None }
}
