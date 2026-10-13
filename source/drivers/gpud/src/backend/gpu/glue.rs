// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The GPU node's SoC glue (RFC-0106, TASK-0329 G1): gpud finds the node in its own read-only
//! tree by compatible and has socd bring it up — domain 2 by its software sequence, `gpu_clk`
//! at the node's rate, the reset released after the clock — before any register is touched.
//! socd's own line names every register it wrote; this one names the verdict.

use nexus_service_topology::slots::gpud as topo;
use nexus_wire::soc;

use super::emit;

/// The tree's compatibles for the core (the stock tree's word first).
pub(super) const COMPATIBLES: [&str; 2] = ["img,rgx", "img,img-rogue"];

/// Bring the node up through socd; `None` after a FAIL line.
pub(super) fn bring_up() -> Option<()> {
    let Some(bytes) = nexus_abi::device_tree::map_read_only(topo::DEVICE_TREE) else {
        emit("gpud: FAIL gpu glue (no device tree)");
        return None;
    };
    let Ok(fdt) = nexus_fdt::Fdt::new(bytes) else {
        emit("gpud: FAIL gpu glue (the tree does not parse)");
        return None;
    };
    let Some(node) = fdt.find_compatible(&COMPATIBLES).next() else {
        emit("gpud: FAIL gpu glue (no gpu node)");
        return None;
    };
    let mut buf = [0u8; 128];
    let Some(path) = node.path_into(&mut buf) else {
        emit("gpud: FAIL gpu glue (a node path longer than the buffer)");
        return None;
    };
    match nexus_ipc::socd::bring_up(topo::SOCD.send, topo::REPLY, path, 0x6701) {
        Ok(reply) if reply.status == soc::STATUS_OK => {
            emit(&alloc::format!(
                "gpud: gpu glue ok (domains={} clocks={} resets={} through socd)",
                reply.domains,
                reply.clocks,
                reply.resets
            ));
            Some(())
        }
        Ok(reply) => {
            emit(&alloc::format!("gpud: FAIL gpu glue ({path} status={})", reply.status));
            None
        }
        Err(error) => {
            emit(&alloc::format!("gpud: FAIL gpu glue ({path} {error:?})"));
            None
        }
    }
}
