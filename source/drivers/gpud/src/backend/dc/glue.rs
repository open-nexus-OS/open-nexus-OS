// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The display nodes' SoC glue (RFC-0106): gpud finds the controller and the encoder in its own
//! read-only tree by compatible and has socd bring each up — power domain 7, `hdmi_reset`,
//! `hmclk` at the rate the nodes demand — before any of their registers is touched. socd's own
//! line names every register it wrote; this one names the verdict.

use nexus_service_topology::slots::gpud as topo;
use nexus_wire::soc;

use super::emit;

/// The board tree's compatibles for the two blocks.
pub(super) const CONTROLLER_COMPATIBLE: &str = "spacemit,dpu-online2";
pub(super) const ENCODER_COMPATIBLE: &str = "spacemit,hdmi";
const PAGE: u64 = 4096;

/// What the tree says beyond the windows init granted: where the encoder's block starts inside
/// the page its window covers.
pub(super) struct Nodes {
    pub encoder_offset: usize,
}

/// Bring both nodes up through socd; `None` after a FAIL line.
pub(super) fn bring_up() -> Option<Nodes> {
    let Some(bytes) = nexus_abi::device_tree::map_read_only(topo::DEVICE_TREE) else {
        emit("gpud: FAIL dc glue (no device tree)");
        return None;
    };
    let Ok(fdt) = nexus_fdt::Fdt::new(bytes) else {
        emit("gpud: FAIL dc glue (the tree does not parse)");
        return None;
    };
    let mut encoder_offset = 0usize;
    for (nonce, compatible) in [(0xDC01u32, CONTROLLER_COMPATIBLE), (0xDC02, ENCODER_COMPATIBLE)] {
        let wanted = [compatible];
        let Some(node) = fdt.find_compatible(&wanted).next() else {
            emit(&alloc::format!("gpud: FAIL dc glue (no {compatible} node)"));
            return None;
        };
        let mut buf = [0u8; 128];
        let Some(path) = node.path_into(&mut buf) else {
            emit("gpud: FAIL dc glue (a node path longer than the buffer)");
            return None;
        };
        match nexus_ipc::socd::bring_up(topo::SOCD.send, topo::REPLY, path, nonce) {
            Ok(reply) if reply.status == soc::STATUS_OK => {}
            Ok(reply) => {
                emit(&alloc::format!("gpud: FAIL dc glue ({path} status={})", reply.status));
                return None;
            }
            Err(error) => {
                emit(&alloc::format!("gpud: FAIL dc glue ({path} {error:?})"));
                return None;
            }
        }
        if compatible == ENCODER_COMPATIBLE {
            let reg = node.reg(0).ok().flatten().map_or(0, |r| r.addr % PAGE);
            encoder_offset = reg as usize;
        }
    }
    emit("gpud: dc glue ok (controller + encoder up through socd)");
    Some(Nodes { encoder_offset })
}
